use crate::{
    aux::profile::{FileMode, Profile},
    fab::localize_path,
};
use anyhow::Result;
use log::warn;
use rayon::prelude::*;
use spawn::{SpawnError, Spawner};
use std::borrow::Cow;
use strum::IntoEnumIterator;

/// Localize and bind
fn localize(mode: FileMode, file: &str, home: bool, handle: &Spawner) -> Result<(), SpawnError> {
    if let (Some(source), dest) = localize_path(file, home) {
        Ok(handle.args_i([Cow::Borrowed(mode.bind()), source, Cow::Owned(dest)])?)
    } else {
        warn!("Failed to resolve: {file}");
        Ok(())
    }
}

pub fn fabricate(profile: &mut Profile, handle: &Spawner) -> Result<()> {
    let saved = user::save()?;
    user::set(user::Mode::Real)?;

    if let Some(files) = &mut profile.files {
        if let Some(mut user_files) = files.user.take() {
            for mode in FileMode::iter() {
                if let Some(files) = user_files.remove(&mode) {
                    files
                        .into_par_iter()
                        .try_for_each(|file| localize(mode, &file, true, handle))
                        .ok();
                }
            }
        }

        if let Some(mut system) = files.system.take() {
            for mode in FileMode::iter() {
                if let Some(files) = system.remove(&mode) {
                    files
                        .into_par_iter()
                        .try_for_each(|file| localize(mode, &file, false, handle))
                        .ok();
                }
            }
        }
    }
    user::restore(saved)?;
    Ok(())
}
