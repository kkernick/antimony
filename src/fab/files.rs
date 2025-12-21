use crate::{
    fab::localize_path,
    shared::{
        env::HOME,
        profile::{FileMode, Profile},
    },
};
use anyhow::Result;
use log::warn;
use rayon::prelude::*;
use spawn::Spawner;
use std::borrow::Cow;
use strum::IntoEnumIterator;

/// Localize and bind
fn localize(mode: FileMode, file: &str, home: bool, handle: &Spawner, can_try: bool) -> Result<()> {
    if let (Some(source), dest) = localize_path(file, home)? {
        Ok(handle.args_i([Cow::Borrowed(mode.bind(can_try)), source, Cow::Owned(dest)])?)
    } else {
        warn!("Failed to resolve: {file}");
        Ok(())
    }
}

pub fn fabricate(profile: &mut Profile, handle: &Spawner) -> Result<()> {
    user::try_run_as!(user::Mode::Real, {
        if let Some(files) = &mut profile.files {
            if let Some(mut user_files) = files.user.take() {
                for mode in FileMode::iter() {
                    if let Some(files) = user_files.remove(&mode) {
                        files
                            .into_par_iter()
                            .try_for_each(|file| {
                                localize(
                                    mode,
                                    &file.replace("~", HOME.as_str()),
                                    true,
                                    handle,
                                    true,
                                )
                            })
                            .ok();
                    }
                }
            }

            if let Some(mut system) = files.platform.take() {
                for mode in FileMode::iter() {
                    if let Some(files) = system.remove(&mode) {
                        files
                            .into_par_iter()
                            .try_for_each(|file| localize(mode, &file, false, handle, true))
                            .ok();
                    }
                }
            }

            if let Some(mut system) = files.resources.take() {
                for mode in FileMode::iter() {
                    if let Some(files) = system.remove(&mode) {
                        files
                            .into_par_iter()
                            .try_for_each(|file| localize(mode, &file, false, handle, false))
                            .ok();
                    }
                }
            }
        }
        Ok(())
    })
}
