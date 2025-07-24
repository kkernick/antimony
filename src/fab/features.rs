use crate::{
    aux::{
        feature::Feature,
        profile::{FileMode, Profile},
    },
    fab::resolve,
};
use log::{debug, warn};
use std::{
    borrow::Cow,
    collections::{BTreeSet, HashMap, HashSet},
};
use strum::IntoEnumIterator;

/// Errors related to feature integration
#[derive(Debug)]
pub enum Error {
    /// Invalid bus name.
    InvalidBus(String),

    /// Feature error.
    Feature(crate::aux::feature::Error),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::InvalidBus(name) => write!(f, "Invalid bus name: {name}"),
            Self::Feature(e) => write!(f, "Failed to parse feature: {e}"),
        }
    }
}
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
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

fn add_feature(
    profile: &mut Profile,
    map: &HashMap<&str, String>,
    mut feature: Feature,
    searched: &mut HashSet<String>,
) -> Result<(), Error> {
    if searched.contains(&feature.name) {
        return Ok(());
    }

    if let Some(caveat) = feature.caveat {
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

        if let Some(mut system) = files.system.take() {
            let p_sys = p_files.system.get_or_insert_default();
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

    if let Some(requires) = feature.requires.take() {
        debug!("Dependencies => {requires:?}");
        for sub in requires {
            add_feature(
                profile,
                map,
                Feature::new(&sub).map_err(Error::Feature)?,
                searched,
            )?;
        }
    }

    searched.insert(feature.name);
    Ok(())
}

pub fn fabricate(profile: &mut Profile, name: &str) -> Result<(), Error> {
    #[rustfmt::skip]
    let map = HashMap::from([
        ("{name}", name.to_string()),
        ("{desktop}", profile.desktop(name).to_string())
    ]);

    let mut searched = HashSet::new();

    if let Some(ref features) = profile.features.take() {
        for feature in features {
            add_feature(
                profile,
                &map,
                Feature::new(feature).map_err(Error::Feature)?,
                &mut searched,
            )?;
        }
    }
    Ok(())
}
