#![allow(clippy::missing_errors_doc)]

use crate::{
    fab::localize_path,
    shared::{
        env::HOME,
        package::Package,
        profile::files::{FILE_MODES, FileMode},
    },
};
use anyhow::Result;
use spawn::Spawner;
use std::borrow::Cow;

/// Localize and bind
#[inline]
pub fn localize(
    mode: FileMode,
    file: &str,
    home: bool,
    handle: &Spawner,
    can_try: bool,
    package: &mut Option<(Package, bool)>,
) -> Result<()> {
    match localize_path(file, home)? {
        (Some(source), dest) => {
            if let Some((package, false)) = package.as_mut() {
                package.add(&source, &dest)?;
            }
            handle.args_i([Cow::Borrowed(mode.bind(can_try)), source, Cow::Owned(dest)]);
        }
        (None, dest) => {
            let resolved = if home && !file.starts_with("/home") {
                Cow::Owned(format!("{}/{file}", HOME.as_str()))
            } else {
                Cow::Borrowed(file)
            };
            handle.args_i([Cow::Borrowed(mode.bind(true)), resolved, Cow::Owned(dest)]);
        }
    }
    Ok(())
}

pub fn fabricate(info: &mut super::FabInfo) -> Result<()> {
    let lockdown = info.profile.lockdown.unwrap_or(false);

    if let Some(files) = &info.profile.files {
        for temp in &files.temp {
            info.handle.args_i(["--tmpfs", temp]);
        }

        for (src, dst) in &files.links {
            info.handle.args_i(["--symlink", src, dst]);
            if let Some((package, false)) = info.package.as_mut() {
                package.add(src, dst)?;
            }
        }

        if !lockdown {
            let user_files = &files.user;
            for mode in FILE_MODES {
                if let Some(files) = user_files.get(&mode) {
                    for file in files {
                        localize(
                            mode,
                            &file.replace('~', HOME.as_str()),
                            true,
                            info.handle,
                            true,
                            &mut None,
                        )?;
                    }
                }
            }
        }

        let system = &files.platform;
        for mode in FILE_MODES {
            if let Some(files) = system.get(&mode) {
                for file in files {
                    localize(mode, file, false, info.handle, true, &mut None)?;
                }
            }
        }

        let system = &files.resources;
        for mode in FILE_MODES {
            if let Some(files) = system.get(&mode) {
                for file in files {
                    localize(mode, file, false, info.handle, false, info.package)?;
                }
            }
        }
    }
    Ok(())
}
