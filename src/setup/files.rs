use crate::{
    fab::{localize_path, resolve_env},
    shared::{path::direct_path, profile::FileMode},
};
use anyhow::Result;
use log::debug;
use rayon::prelude::*;
use spawn::Spawner;
use std::{
    borrow::Cow,
    fs::{self, File},
};
use strum::IntoEnumIterator;
use user::{self, try_run_as};

fn get_x(file: &str, handle: &Spawner) -> Result<()> {
    handle.fd_arg_i("--file", File::open(file)?)?;
    handle.arg_i(file)?;
    handle.args_i(["--chmod", "555", file])?;
    Ok(())
}

pub fn add_file(handle: &Spawner, file: &str, contents: String, op: FileMode) -> Result<()> {
    let path = direct_path(file);
    if !path.exists() {
        fs::create_dir_all(path.parent().unwrap())?;
        let contents = resolve_env(Cow::Borrowed(&contents));
        fs::write(&path, contents.as_ref())?;
    }

    handle.fd_arg_i("--file", File::open(path)?)?;
    handle.arg_i(file)?;
    handle.args_i(["--chmod", op.chmod(), file])?;
    Ok(())
}

pub fn setup(args: &mut super::Args) -> Result<()> {
    debug!("Setting up files");
    // Add direct files.
    if let Some(files) = &mut args.profile.files {
        if let Some(user) = &mut files.user
            && let Some(exe) = user.remove(&FileMode::Executable)
        {
            try_run_as!(
                user::Mode::Real,
                exe.into_par_iter().try_for_each(|file| {
                    let (_, dest) = localize_path(&file, true)?;
                    get_x(&dest, &args.handle)
                })
            )?;
        }
        if let Some(system) = &mut files.platform
            && let Some(exe) = system.remove(&FileMode::Executable)
        {
            exe.into_par_iter()
                .try_for_each(|file| get_x(&file, &args.handle))?;
        }

        if let Some(system) = &mut files.resources
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
