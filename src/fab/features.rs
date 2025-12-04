use crate::{
    shared::{
        feature::Feature,
        profile::{FileMode, Profile},
    },
    fab::resolve,
};
use log::{debug, warn};
use spawn::Spawner;
use std::{
    borrow::Cow,
    collections::{BTreeSet, HashMap, HashSet},
    error, fmt,
};
use strum::IntoEnumIterator;

/// Errors related to feature integration
#[derive(Debug)]
pub enum Error {
    /// Invalid bus name.
    InvalidBus(String),

    /// Feature error.
    Feature(crate::shared::feature::Error),
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::InvalidBus(name) => write!(f, "Invalid bus name: {name}"),
            Self::Feature(e) => write!(f, "Failed to parse feature: {e}"),
        }
    }
}
impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Feature(e) => Some(e),
            _ => None,
        }
    }
}

/// Replace {} names with the real values in the profile.
fn format(mut str: String, map: &HashMap<&str, String>) -> Result<String, Error> {
    for (key, val) in map {
        str = str.replace(key, val);
    }

    if !str.contains('.') {
        Err(Error::InvalidBus(str))
    } else {
        Ok(str)
    }
}

/// Load a feature from the cache database. This prevents loading the same Feature multiple times.
fn load_feature<'a>(
    name: &str,
    db: &'a mut HashMap<String, Feature>,
) -> Result<&'a mut Feature, Error> {
    Ok(db
        .entry(name.to_string())
        .or_insert(Feature::new(name).map_err(Error::Feature)?))
}

/// Strike a feature from the feature list.
/// This function operates with the following logic:
///
/// 1. If this feature is currently required, it is removed from the requirement list *immediately*
/// 2. Every feature that this feature depends on is decremented in the requirement list.
/// 3. If those dependencies were only required by the stricken feature, they are also stricken.
///
/// This ensures that when a feature is deemed a conflict with another feature, both it, and any
/// dependencies that were only required by the now stricken feature, are removed.
///
/// Remember, conflicting features supersede everything else. No matter where a conflicting feature
/// is defined, it will be removed from the set regardless of where the conflict exists, how many
/// other features rely on, etc.
fn strike_feature(
    feature: &str,
    db: &mut HashMap<String, Feature>,
    features: &mut HashMap<String, u32>,
) -> Result<(), Error> {
    // If we required this feature
    if features.contains_key(feature) {
        debug!("Striking feature: {feature}");

        // Remove the offending feature immediately.
        features.remove(feature);

        // Then, grab the features that this feature requires.
        if let Some(depends) = load_feature(feature, db)?.requires.clone() {
            for depend in depends {
                // For each, decrement the dependency count.
                if let Some(feat) = features.get_mut(&depend) {
                    *feat -= 1;

                    // If this feature was the only one requiring this dependency, strike it as well.
                    if *feat < 1 {
                        strike_feature(&depend, db, features)?;
                    }
                }
            }
        }
    }
    Ok(())
}

/// Resolves features. This function recursively resolves each feature, and all the required features
/// it needs. It also excludes any conflicts, with intelligent dependency sorting.
fn resolve_feature(
    feature: &str,
    db: &mut HashMap<String, Feature>,
    features: &mut HashMap<String, u32>,
    blacklist: &mut BTreeSet<String>,
    searched: &mut HashSet<String>,
) -> Result<(), Error> {
    // If we haven't search this already.
    if !searched.contains(feature) && !blacklist.contains(feature) {
        // Add this feature to our feature list if it doesn't exit.
        *features.entry(feature.to_string()).or_insert(0) += 1;

        // Add to searched.
        searched.insert(feature.to_string());

        // Get a copy of the required features, and conflicting features.
        let (requires, conflicts) = {
            match load_feature(feature, db) {
                Ok(feature) => (feature.requires.clone(), feature.conflicts.clone()),
                Err(_) => (None, None),
            }
        };

        // Resolve the requirements.
        if let Some(requires) = requires {
            for require in requires {
                resolve_feature(&require, db, features, blacklist, searched)?;
            }
        }

        // Strike out conflicts.
        if let Some(conflicts) = conflicts {
            blacklist.extend(conflicts.clone());
            for conflict in conflicts {
                if features.contains_key(&conflict) {
                    strike_feature(&conflict, db, features)?;
                }
            }
        }
    }
    Ok(())
}

fn resolve_features(
    profile: &mut Profile,
    db: &mut HashMap<String, Feature>,
) -> Result<HashSet<String>, Error> {
    let mut features = HashMap::new();
    let mut searched = HashSet::new();
    let mut blacklist = profile.conflicts.take().unwrap_or_default();

    if let Some(feats) = &profile.features {
        for feat in feats {
            resolve_feature(
                feat.as_str(),
                db,
                &mut features,
                &mut blacklist,
                &mut searched,
            )?;
        }
    }
    Ok(features.into_keys().collect())
}

fn add_feature(
    profile: &mut Profile,
    map: &HashMap<&str, String>,
    feature: &mut Feature,
) -> Result<(), Error> {
    if let Some(condition) = feature.conditional.take() {
        let code = || -> anyhow::Result<i32> {
            let code = Spawner::new("/usr/bin/bash")
                .args(["-c", &condition])?
                .preserve_env(true)
                .mode(user::Mode::Real)
                .output(true)
                .error(true)
                .spawn()?
                .wait()?;
            Ok(code)
        }();

        match code {
            Ok(code) => {
                if code != 0 {
                    debug!("Condition for feature {} not met", &feature.name);
                    return Ok(());
                }
            }
            Err(e) => {
                debug!(
                    "Failed to check condition for feature {}: {e}",
                    &feature.name
                );
                return Ok(());
            }
        }
    }

    if let Some(caveat) = feature.caveat.take() {
        warn!(
            "This profile uses a dangerous feature! {}: {}",
            feature.name, caveat
        );
    } else {
        debug!("Adding feature: {}", feature.name);
    }

    if let Some(mut files) = feature.files.take() {
        let p_files = profile.files.get_or_insert_default();

        if let Some(mut direct) = files.direct.take() {
            let p_direct = p_files.direct.get_or_insert_default();
            for mode in FileMode::iter() {
                if let Some(d_files) = direct.remove(&mode) {
                    p_direct.entry(mode).or_default().extend(d_files);
                };
            }
        }

        if let Some(mut system) = files.platform.take() {
            let p_sys = p_files.platform.get_or_insert_default();
            for mode in FileMode::iter() {
                if let Some(sys_files) = system.remove(&mode) {
                    p_sys.entry(mode).or_default().extend(
                        sys_files
                            .into_iter()
                            .map(|s| resolve(Cow::Owned(s)).into_owned()),
                    );
                }
            }
        }

        if let Some(mut system) = files.resources.take() {
            let p_sys = p_files.resources.get_or_insert_default();
            for mode in FileMode::iter() {
                if let Some(sys_files) = system.remove(&mode) {
                    p_sys.entry(mode).or_default().extend(
                        sys_files
                            .into_iter()
                            .map(|s| resolve(Cow::Owned(s)).into_owned()),
                    );
                }
            }
        }

        if let Some(mut user) = files.user.take() {
            let p_user = p_files.user.get_or_insert_default();

            for mode in FileMode::iter() {
                if let Some(user_files) = user.remove(&mode) {
                    p_user.entry(mode).or_default().extend(
                        user_files
                            .into_iter()
                            .map(|s| resolve(Cow::Owned(s)).into_owned()),
                    );
                }
            }
        }
    }

    if let Some(binaries) = feature.binaries.take() {
        profile.binaries.get_or_insert_default().extend(binaries);
    }
    if let Some(libraries) = feature.libraries.take() {
        profile.libraries.get_or_insert_default().extend(libraries);
    }
    if let Some(devices) = feature.devices.take() {
        profile.devices.get_or_insert_default().extend(devices);
    }
    if let Some(namespaces) = feature.namespaces.take() {
        profile
            .namespaces
            .get_or_insert_default()
            .extend(namespaces);
    }
    if let Some(args) = feature.sandbox_args.take() {
        profile.sandbox_args.get_or_insert_default().extend(args);
    }

    if let Some(mut ipc) = feature.ipc.take() {
        let format_all = |ipc_list: BTreeSet<String>| -> BTreeSet<String> {
            ipc_list
                .into_iter()
                .filter_map(|f| format(f, map).ok())
                .collect()
        };

        if !ipc.portals.is_empty() {
            profile
                .ipc
                .get_or_insert_default()
                .portals
                .append(&mut ipc.portals);
        }
        if !ipc.see.is_empty() {
            let formatted = format_all(ipc.see);
            if !formatted.is_empty() {
                profile.ipc.get_or_insert_default().see.extend(formatted);
            }
        }
        if !ipc.talk.is_empty() {
            let mut formatted = format_all(ipc.talk);
            if !formatted.is_empty() {
                profile
                    .ipc
                    .get_or_insert_default()
                    .talk
                    .append(&mut formatted);
            }
        }
        if !ipc.own.is_empty() {
            let mut formatted = format_all(ipc.own);
            if !formatted.is_empty() {
                profile
                    .ipc
                    .get_or_insert_default()
                    .own
                    .append(&mut formatted);
            }
        }
        if !ipc.call.is_empty() {
            profile
                .ipc
                .get_or_insert_default()
                .call
                .append(&mut ipc.call);
        }
    }

    if let Some(env) = feature.environment.take() {
        for (key, value) in env {
            profile
                .environment
                .get_or_insert_default()
                .insert(key, resolve(Cow::Owned(value)).into_owned());
        }
    }
    Ok(())
}

pub fn fabricate(profile: &mut Profile, name: &str) -> Result<(), Error> {
    #[rustfmt::skip]
    let map = HashMap::from([
        ("{name}", name.to_string()),
        ("{desktop}", profile.desktop(name).to_string())
    ]);

    let mut db = HashMap::new();
    for feature in resolve_features(profile, &mut db)? {
        add_feature(profile, &map, load_feature(&feature, &mut db)?)?;
    }
    Ok(())
}
