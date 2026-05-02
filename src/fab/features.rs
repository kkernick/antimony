#![allow(clippy::missing_errors_doc)]
//! The Feature Fabricator composes all defined features defined in the Profile,
//! recursively analyzes dependencies, strikes conflicts, and merges definitions
//! into a single, complete Profile.
//!
//! Note that due to performance considerations, the feature fabricator is called on
//! `Profile::new()`, and is cached separately from the other fabricators as well.

use crate::{
    fab::resolve,
    shared::{
        Map, Set,
        feature::{self, Feature},
        profile::{Profile, files::FILE_MODES},
    },
};
use log::{debug, warn};
use spawn::{Spawner, StreamMode};
use std::{borrow::Cow, num::Saturating};
use thiserror::Error;

/// Errors related to feature integration
#[derive(Debug, Error)]
pub enum Error {
    /// Invalid bus name.
    #[error("Invalid bus name: {0}")]
    InvalidBus(String),

    /// Feature error.
    #[error("Feature error: {0}")]
    Feature(feature::Error),
}

/// Replace {} names with the real values in the profile.
#[inline]
fn format(mut str: String, map: &Map<&str, String>) -> Result<String, Error> {
    for (key, val) in map {
        str = str.replace(key, val);
    }

    if str.contains('.') {
        Ok(str)
    } else {
        Err(Error::InvalidBus(str))
    }
}

/// Load a feature from the cache database. This prevents loading the same Feature multiple times.
#[inline]
fn load_feature<'a>(name: &str, db: &'a mut Map<String, Feature>) -> Result<&'a Feature, Error> {
    Ok(db
        .entry(name.to_owned())
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
    db: &mut Map<String, Feature>,
    features: &mut Map<String, Saturating<u32>>,
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
                    if *feat < Saturating(1) {
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
    db: &mut Map<String, Feature>,
    features: &mut Map<String, Saturating<u32>>,
    blacklist: &mut Set<String>,
    searched: &mut Set<String>,
) -> Result<(), Error> {
    // If we haven't search this already.
    if !searched.contains(feature) && !blacklist.contains(feature) {
        // Add this feature to our feature list if it doesn't exit.
        *features.entry(feature.to_owned()).or_default() += 1;

        // Add to searched.
        searched.insert(feature.to_owned());

        // Get a copy of the required features, and conflicting features.
        let (requires, conflicts) = {
            match load_feature(feature, db) {
                Ok(feature) => (feature.requires.clone(), feature.conflicts.clone()),
                Err(e) => {
                    warn!("Could not load feature {feature}: {e}");
                    (None, None)
                }
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

/// Resolve the final feature set, accounting for conflicts
fn resolve_features(
    features: &Set<String>,
    conflicts: &Set<String>,
) -> Result<Set<Feature>, Error> {
    let mut feature_list = Map::default();
    let mut searched = Set::default();
    let mut blacklist = conflicts.clone();
    let mut db = Map::default();

    for feat in features {
        resolve_feature(
            feat.as_str(),
            &mut db,
            &mut feature_list,
            &mut blacklist,
            &mut searched,
        )?;
    }

    Ok(feature_list
        .into_keys()
        .filter_map(|name| db.remove(&name))
        .collect())
}

/// Add the definitions of a feature to a profile.
#[allow(clippy::too_many_lines)]
fn add_feature(profile: &mut Profile, map: &Map<&str, String>, mut feature: Feature) {
    // Conditionals intentionally don't use any schema. It just runs the content through
    // bash. This lets you do something as simple requiring a certain file via `command`, or
    // something more complicated. It runs under the
    if let Some(condition) = feature.conditional.take() {
        let code = || -> anyhow::Result<i32> {
            let code = Spawner::abs("/usr/bin/bash")
                .args(["-c", &condition])
                .preserve_env(true)
                .mode(user::Mode::Real)
                .output(StreamMode::Discard)
                .error(StreamMode::Discard)
                .spawn()?
                .wait()?;
            Ok(code)
        }();

        match code {
            Ok(code) => {
                if code != 0 {
                    debug!("Condition for feature {} not met", &feature.name);
                }
            }
            Err(e) => {
                debug!(
                    "Failed to check condition for feature {}: {e}",
                    &feature.name
                );
            }
        }
    }

    // Caveats don't do anything more than warn. If a feature was truly dangerous enough to warrant an error,
    // it wouldn't be acceptable to use in Antimony at all. Remember, it can't be any worse than running the
    // application on the host directly. Even the broad features like `lib` and `bin` restrict access to
    // other processes, the possibility for root access, etc.
    if let Some(caveat) = feature.caveat.take() {
        warn!(
            "This profile uses a dangerous feature! {}: {}",
            feature.name, caveat
        );
    } else {
        debug!("Adding feature: {}", feature.name);
    }

    if let Some(files) = feature.files.take() {
        let p_files = profile.files.get_or_insert_default();

        let mut direct = files.direct;
        let p_direct = &mut p_files.direct;
        for mode in FILE_MODES {
            if let Some(d_files) = direct.remove(&mode) {
                p_direct.entry(mode).or_default().extend(d_files);
            }
        }

        let mut system = files.platform;
        let p_sys = &mut p_files.platform;
        for mode in FILE_MODES {
            if let Some(sys_files) = system.remove(&mode) {
                p_sys.entry(mode).or_default().extend(
                    sys_files
                        .into_iter()
                        .map(|s| resolve(Cow::Owned(s)).into_owned()),
                );
            }
        }

        let mut system = files.resources;
        let p_sys = &mut p_files.resources;
        for mode in FILE_MODES {
            if let Some(sys_files) = system.remove(&mode) {
                p_sys.entry(mode).or_default().extend(
                    sys_files
                        .into_iter()
                        .map(|s| resolve(Cow::Owned(s)).into_owned()),
                );
            }
        }

        let mut user = files.user;
        let p_user = &mut p_files.user;

        for mode in FILE_MODES {
            if let Some(user_files) = user.remove(&mode) {
                p_user.entry(mode).or_default().extend(
                    user_files
                        .into_iter()
                        .map(|s| resolve(Cow::Owned(s)).into_owned()),
                );
            }
        }
    }

    if let Some(binaries) = feature.binaries.take() {
        profile.binaries.extend(binaries);
    }
    if let Some(devices) = feature.devices.take() {
        profile.devices.extend(devices);
    }
    if let Some(namespaces) = feature.namespaces.take() {
        profile.namespaces.extend(namespaces);
    }
    if let Some(args) = feature.sandbox_args.take() {
        profile.sandbox_args.extend(args);
    }

    if let Some(libraries) = feature.libraries.take() {
        let p_lib = profile.libraries.get_or_insert_default();
        p_lib.directories.extend(libraries.directories);
        p_lib.files.extend(libraries.files);
        p_lib.roots.extend(libraries.roots);
        p_lib.no_sof = match p_lib.no_sof {
            Some(false) | None => libraries.no_sof,
            Some(true) => Some(true),
        };
    }

    if let Some(ipc) = feature.ipc.take() {
        let p_ipc = profile.ipc.get_or_insert_default();

        // Features, as the name implies, *add* functionality. Antimony
        // operates under a secure-default, so things need to be added.
        // Because single-values can conflict with each other, this
        // design choice is reflected in setting behavior.

        // Features can enable the global bus flags if they need it,
        // but they cannot try and restrict the bus if another feature/profile
        // has turned it on.
        p_ipc.system_bus = match p_ipc.system_bus {
            Some(false) | None => ipc.system_bus,
            Some(true) => Some(true),
        };
        p_ipc.user_bus = match p_ipc.user_bus {
            Some(false) | None => ipc.user_bus,
            Some(true) => Some(true),
        };

        // Conversely, if a feature or profile has explicitly set
        // disable to false for compatibility, you cannot enable it.
        p_ipc.disable = match p_ipc.disable {
            Some(true) => {
                if ipc.disable.is_some() {
                    ipc.disable
                } else {
                    Some(true)
                }
            }
            Some(false) => Some(false),
            None => ipc.disable,
        };

        let format_all = |ipc_list: Set<String>| -> Set<String> {
            ipc_list
                .into_iter()
                .filter_map(|f| format(f, map).ok())
                .collect()
        };

        if !ipc.portals.is_empty() {
            p_ipc.portals.extend(ipc.portals);
        }
        if !ipc.sees.is_empty() {
            let formatted = format_all(ipc.sees);
            if !formatted.is_empty() {
                p_ipc.sees.extend(formatted);
            }
        }
        if !ipc.talks.is_empty() {
            let formatted = format_all(ipc.talks);
            if !formatted.is_empty() {
                p_ipc.talks.extend(formatted);
            }
        }
        if !ipc.owns.is_empty() {
            let formatted = format_all(ipc.owns);
            if !formatted.is_empty() {
                p_ipc.owns.extend(formatted);
            }
        }
        if !ipc.calls.is_empty() {
            p_ipc.calls.extend(ipc.calls);
        }
    }

    if let Some(env) = feature.environment.take() {
        for (key, value) in env {
            profile
                .environment
                .insert(key, resolve(Cow::Owned(value)).into_owned());
        }
    }

    if let Some(mut hooks) = feature.hooks.take() {
        let p_hooks = profile.hooks.get_or_insert_default();
        p_hooks.pre.append(&mut hooks.pre);
        p_hooks.post.append(&mut hooks.post);

        // Yeah, no great answer here. The last feature that sets a parent
        // hook get it.
        if p_hooks.parent.is_none() {
            p_hooks.parent = hooks.parent;
        }
    }

    profile.new_privileges = match profile.new_privileges {
        Some(false) | None => feature.new_privileges,
        Some(true) => Some(true),
    };
}

#[allow(clippy::literal_string_with_formatting_args)]
pub fn fabricate(profile: &mut Profile, name: &str) -> Result<(), Error> {
    let mut map = Map::default();
    map.insert("{name}", name.to_owned());
    map.insert("{desktop}", profile.desktop(name).to_string());

    for feature in resolve_features(&profile.features, &profile.conflicts)? {
        add_feature(profile, &map, feature);
    }
    Ok(())
}
