#![allow(clippy::missing_errors_doc)]

use crate::{
    fab::localize_path,
    shared::{
        env::HOME,
        landlock::{LandlockPolicy, RO, RW, update_policy},
        package::Package,
        profile::files::{FILE_MODES, FileMode},
    },
};
use anyhow::Result;
use landlock::ruleset::Filesystem;
use spawn::Spawner;
use std::{borrow::Cow, path::Path};

/// Localize and bind
#[inline]
pub fn localize(
    mode: FileMode,
    file: &str,
    home: bool,
    handle: &Spawner,
    can_try: bool,
    package: &mut Option<(Package, bool)>,
    policy: &mut Option<LandlockPolicy>,
) -> Result<()> {
    let (src, dest) = localize_path(file, home)?;
    if let Some(source) = src {
        if let Some((package, false)) = package.as_mut() {
            package.add(&source, &dest)?;
        } else {
            handle.args_i([
                Cow::Borrowed(mode.bind(can_try)),
                source,
                Cow::Borrowed(&dest),
            ]);
        }
    } else {
        let resolved = if home && !file.starts_with("/home") {
            Cow::Owned(format!("{}/{file}", HOME.as_str()))
        } else {
            Cow::Borrowed(file)
        };
        handle.args_i([
            Cow::Borrowed(mode.bind(true)),
            resolved,
            Cow::Borrowed(&dest),
        ]);
    }

    if let Some(policy) = policy
        && let Some(parent) = Path::new(&dest).parent()
    {
        match mode {
            FileMode::ReadOnly => update_policy(parent, policy, RO),
            FileMode::ReadWrite => update_policy(parent, policy, RW),
            FileMode::Executable => {
                update_policy(parent, policy, [Filesystem::ReadFile, Filesystem::Execute]);
            }
        }
    }

    Ok(())
}

pub fn fabricate(info: &mut super::FabInfo) -> Result<()> {
    let lockdown = info.profile.lockdown.unwrap_or(false);

    if let Some(files) = &info.profile.files {
        for temp in &files.temp {
            let (_, dest) = localize_path(temp, false)?;
            info.handle.args_i(["--tmpfs", &dest]);
            if let Some(policy) = info.policy {
                update_policy(dest, policy, RW);
            }
        }

        for (src, dst) in &files.links {
            if let Some((package, false)) = info.package.as_mut() {
                package.add(src, dst)?;
            } else {
                info.handle.args_i(["--symlink", src, dst]);
                if let Some(policy) = info.policy
                    && let Some(parent) = Path::new(dst).parent()
                {
                    update_policy(parent, policy, RO);
                }
            }
        }

        if !lockdown {
            let user_files = &files.user;
            for mode in FILE_MODES {
                if let Some(files) = user_files.get(&mode) {
                    for file in files {
                        localize(
                            mode,
                            &file.replacen('~', HOME.as_str(), 1),
                            true,
                            info.handle,
                            true,
                            &mut None,
                            info.policy,
                        )?;
                    }
                }
            }
        }

        if let Some(policy) = info.policy {
            for (path, mode) in &files.permissions {
                let (_, dest) = localize_path(path, false)?;
                match mode {
                    FileMode::ReadOnly => update_policy(dest, policy, RO),
                    FileMode::ReadWrite => update_policy(dest, policy, RW),
                    FileMode::Executable => update_policy(
                        dest,
                        policy,
                        [
                            Filesystem::ReadDir,
                            Filesystem::ReadFile,
                            Filesystem::Execute,
                        ],
                    ),
                }
            }
        }

        let system = &files.platform;
        for mode in FILE_MODES {
            if let Some(files) = system.get(&mode) {
                for file in files {
                    localize(mode, file, false, info.handle, true, &mut None, info.policy)?;
                }
            }
        }

        if info.package.as_ref().map_or_else(|| true, |(_, b)| !b) {
            let system = &files.resources;
            for mode in FILE_MODES {
                if let Some(files) = system.get(&mode) {
                    for file in files {
                        localize(
                            mode,
                            file,
                            false,
                            info.handle,
                            false,
                            info.package,
                            info.policy,
                        )?;
                    }
                }
            }
        }
    }
    Ok(())
}
