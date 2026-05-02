#![allow(clippy::missing_errors_doc)]

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

pub fn fabricate(info: &super::FabInfo) -> Result<()> {
    let lockdown = info.profile.lockdown.unwrap_or(false);

    if let Some(files) = &info.profile.files {
        for temp in &files.temp {
            info.handle.args_i(["--tmpfs", temp]);
        }

        if !lockdown {
            let user_files = &files.user;
            for mode in FILE_MODES {
                if let Some(files) = user_files.get(&mode) {
                    files.into_par_iter().try_for_each(|file| {
                        localize(
                            mode,
                            &file.replace('~', HOME.as_str()),
                            true,
                            info.handle,
                            true,
                        )
                    })?;
                }
            }
        }

        let system = &files.platform;
        for mode in FILE_MODES {
            if let Some(files) = system.get(&mode) {
                files
                    .into_par_iter()
                    .try_for_each(|file| localize(mode, file, false, info.handle, true))?;
            }
        }

        let system = &files.resources;
        for mode in FILE_MODES {
            if let Some(files) = system.get(&mode) {
                files
                    .into_par_iter()
                    .try_for_each(|file| localize(mode, file, false, info.handle, false))?;
            }
        }
    }
    Ok(())
}
