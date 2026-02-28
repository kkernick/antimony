use crate::{
    fab::localize_path,
    shared::{
        env::HOME,
        profile::files::{FILE_MODES, FileMode},
    },
};
use anyhow::Result;
use rayon::prelude::*;
use spawn::Spawner;
use std::borrow::Cow;

/// Localize and bind
#[inline]
fn localize(mode: FileMode, file: &str, home: bool, handle: &Spawner, can_try: bool) -> Result<()> {
    match localize_path(file, home)? {
        (Some(source), dest) => {
            Ok(handle.args_i([Cow::Borrowed(mode.bind(can_try)), source, Cow::Owned(dest)])?)
        }
        (None, dest) => {
            let resolved = if home && !file.starts_with("/home") {
                Cow::Owned(format!("{}/{file}", HOME.as_str()))
            } else {
                Cow::Borrowed(file)
            };

            Ok(handle.args_i([Cow::Borrowed(mode.bind(true)), resolved, Cow::Owned(dest)])?)
        }
    }
}

pub fn fabricate(info: &super::FabInfo) -> Result<()> {
    if let Some(files) = &info.profile.lock().files {
        for temp in &files.temp {
            info.handle.args_i(["--tmpfs", temp])?;
        }

        let user_files = &files.user;
        for mode in FILE_MODES {
            if let Some(files) = user_files.get(&mode) {
                files
                    .into_par_iter()
                    .try_for_each(|file| {
                        localize(
                            mode,
                            &file.replace("~", HOME.as_str()),
                            true,
                            info.handle,
                            true,
                        )
                    })
                    .ok();
            }
        }

        let system = &files.platform;
        for mode in FILE_MODES {
            if let Some(files) = system.get(&mode) {
                files
                    .into_par_iter()
                    .try_for_each(|file| localize(mode, file, false, info.handle, true))
                    .ok();
            }
        }

        let system = &files.resources;
        for mode in FILE_MODES {
            if let Some(files) = system.get(&mode) {
                files
                    .into_par_iter()
                    .try_for_each(|file| localize(mode, file, false, info.handle, false))
                    .ok();
            }
        }
    }
    Ok(())
}
