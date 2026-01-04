use crate::{
    fab::localize_path,
    shared::{
        env::HOME,
        profile::{FILE_MODES, FileMode},
    },
};
use anyhow::Result;
use log::warn;
use rayon::prelude::*;
use spawn::Spawner;
use std::borrow::Cow;

/// Localize and bind
#[inline]
fn localize(mode: FileMode, file: &str, home: bool, handle: &Spawner, can_try: bool) -> Result<()> {
    if let (Some(source), dest) = localize_path(file, home)? {
        Ok(handle.args_i([Cow::Borrowed(mode.bind(can_try)), source, Cow::Owned(dest)])?)
    } else {
        warn!("Failed to resolve: {file}");
        Ok(())
    }
}

pub fn fabricate(info: &super::FabInfo) -> Result<()> {
    if let Some(files) = info.profile.lock().files.take() {
        let mut user_files = files.user;
        for mode in FILE_MODES {
            if let Some(files) = user_files.swap_remove(&mode) {
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

        let mut system = files.platform;
        for mode in FILE_MODES {
            if let Some(files) = system.swap_remove(&mode) {
                files
                    .into_par_iter()
                    .try_for_each(|file| localize(mode, &file, false, info.handle, true))
                    .ok();
            }
        }

        let mut system = files.resources;
        for mode in FILE_MODES {
            if let Some(files) = system.swap_remove(&mode) {
                files
                    .into_par_iter()
                    .try_for_each(|file| localize(mode, &file, false, info.handle, false))
                    .ok();
            }
        }
    }
    Ok(())
}
