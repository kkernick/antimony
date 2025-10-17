use crate::{
    aux::{path::direct_path, profile::FileMode},
    fab::{localize_path, resolve_env},
};
use anyhow::Result;
use log::debug;
use rayon::prelude::*;
use spawn::Spawner;
use std::{borrow::Cow, fs::File};
use strum::IntoEnumIterator;
use user;

fn get_x(file: &str, handle: &Spawner) -> Result<()> {
    handle.fd_arg_i("--file", File::open(file)?)?;
    handle.arg_i(file)?;
    handle.args_i(["--chmod", "555", file])?;
    Ok(())
}

pub fn add_file(handle: &Spawner, file: &str, contents: String, op: FileMode) -> Result<()> {
    let saved = user::save()?;
    user::set(user::Mode::Effective)?;

    let path = direct_path(file);

    if !path.exists() {
        std::fs::create_dir_all(path.parent().unwrap())?;
        let contents = resolve_env(Cow::Borrowed(&contents));
        std::fs::write(&path, contents.as_ref())?;
    }

    handle.fd_arg_i("--file", File::open(path)?)?;
    handle.arg_i(file)?;
    handle.args_i(["--chmod", op.chmod(), file])?;

    user::restore(saved)?;
    Ok(())
}

pub fn setup(args: &mut super::Args) -> Result<()> {
    debug!("Setting up files");
    // Add direct files.
    if let Some(files) = &mut args.profile.files {
        if let Some(user) = &mut files.user
            && let Some(exe) = user.remove(&FileMode::Executable)
        {
            user::set(user::Mode::Real)?;
            exe.into_par_iter().try_for_each(|file| {
                let (_, dest) = localize_path(&file, true);
                get_x(&dest, &args.handle)
            })?;

            user::revert()?;
        }
        if let Some(system) = &mut files.system
            && let Some(exe) = system.remove(&FileMode::Executable)
        {
            exe.into_par_iter()
                .try_for_each(|file| get_x(&file, &args.handle))?;
        }

        if let Some(direct) = &files.direct {
            debug!("Creating direct files");
            for mode in FileMode::iter() {
                if let Some(files) = direct.get(&mode) {
                    files.into_par_iter().try_for_each(|(file, contents)| {
                        add_file(&args.handle, file, contents.clone(), mode)
                    })?;
                }
            }
        };
    }
    Ok(())
}
