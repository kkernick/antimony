use crate::{
    fab::{localize_path, resolve_env},
    shared::{
        direct_path,
        profile::{
            files::{FILE_MODES, FileMode},
            home::HomePolicy,
        },
    },
};
use anyhow::Result;
use log::{debug, warn};
use rayon::prelude::*;
use spawn::Spawner;
use std::{
    borrow::Cow,
    fs::{self, File},
    os::fd::{AsRawFd, OwnedFd},
    path::Path,
    sync::Arc,
};
use user::{USER, as_effective, as_real};

/// Open a file and pass it as executable to the sandbox.
#[inline]
fn get_x<'a>(src: Option<Cow<'a, str>>, dst: &str, handle: &Spawner) -> Result<()> {
    let src = src.unwrap_or(Cow::Borrowed(dst));
    let fd = OwnedFd::from(File::open(src.as_ref())?);
    handle.args_i(["--file", &format!("{}", fd.as_raw_fd()), dst])?;
    handle.fd_i(fd);
    handle.args_i(["--chmod", "555", dst])?;
    Ok(())
}

#[inline]
fn lockdown_file<'a>(
    src: Option<Cow<'a, str>>,
    dst: &str,
    handle: &Spawner,
    ro: bool,
) -> Result<()> {
    let src = src.unwrap_or(Cow::Borrowed(dst));
    if !Path::new(src.as_ref()).is_file() {
        return Err(anyhow::anyhow!(
            "Lockdown can only pass user files. {src} is not a file! Profile cannot use Lockdown!"
        ));
    }

    let fd = match as_real!({ File::open(src.as_ref()) })? {
        Ok(file) => OwnedFd::from(file),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            warn!("No such file: {src}");
            return Ok(());
        }
        Err(e) => {
            return Err(anyhow::anyhow!(
                "Could not open file {src}: {e}. This profile cannot use Lockdown!"
            ));
        }
    };

    handle.args_i(["--file", &format!("{}", fd.as_raw_fd()), dst])?;
    handle.fd_i(fd);
    handle.args_i(["--chmod", if ro { "444" } else { "666" }, dst])?;
    Ok(())
}

/// Add a file to the sandbox.
pub fn add_file(handle: &Spawner, file: &str, contents: String, op: FileMode) -> Result<()> {
    let path = direct_path(file);
    let fd = as_effective!(Result<OwnedFd>, {
        if !path.exists()
            && let Some(parent) = path.parent()
        {
            fs::create_dir_all(parent)?;
            let contents = resolve_env(Cow::Borrowed(&contents));
            fs::write(&path, contents.as_ref())?;
        }

        Ok(OwnedFd::from(File::open(path)?))
    })??;

    handle.args_i(["--file", &format!("{}", fd.as_raw_fd()), file])?;
    handle.fd_i(fd);
    handle.args_i(["--chmod", op.chmod(), file])?;
    Ok(())
}

pub fn setup(args: &Arc<super::Args>) -> Result<()> {
    debug!("Setting up files");
    // Add direct files.

    // Grab the lockdown value
    let lockdown = args.profile.lock().lockdown.unwrap_or(false);

    if lockdown
        && let Some(home) = &args.profile.lock().home
        && let Some(policy) = home.policy
        && policy == HomePolicy::Enabled
    {
        let path = format!(
            "/usr/share/antimony/lockdown/{}/{}",
            USER.real.as_raw(),
            args.name
        );
        args.handle.env_i("LOCKDOWN_HOME", &path)?;
        args.handle.args_i(["--bind", &path, "/home/antimony"])?;
    }

    if let Some(files) = &mut args.profile.lock().files {
        let user = &mut files.user;
        if let Some(exe) = user.swap_remove(&FileMode::Executable) {
            exe.into_par_iter().try_for_each(|file| {
                let (src, dest) = localize_path(&file, true)?;
                get_x(src, &dest, &args.handle)
            })?;
        }

        // Lockdown takes all user files and passes them as FDs. Folders are not supported.
        if lockdown {
            if let Some(ro) = user.swap_remove(&FileMode::ReadOnly) {
                ro.into_par_iter().try_for_each(|file| {
                    let (src, dest) = localize_path(&file, true)?;
                    lockdown_file(src, &dest, &args.handle, true)
                })?;
            }
            if let Some(rw) = user.swap_remove(&FileMode::ReadWrite) {
                rw.into_par_iter().try_for_each(|file| {
                    let (src, dest) = localize_path(&file, true)?;
                    lockdown_file(src, &dest, &args.handle, false)
                })?;
            }
        }

        let system = &mut files.platform;
        if let Some(exe) = system.swap_remove(&FileMode::Executable) {
            exe.into_par_iter()
                .try_for_each(|file| get_x(None, &file, &args.handle))?;
        }

        let system = &mut files.resources;
        if let Some(exe) = system.swap_remove(&FileMode::Executable) {
            exe.into_par_iter()
                .try_for_each(|file| get_x(None, &file, &args.handle))?;
        }

        let direct = &mut files.direct;
        debug!("Creating direct files");
        for mode in FILE_MODES {
            if let Some(files) = direct.get(&mode) {
                files.into_par_iter().try_for_each(|(file, contents)| {
                    add_file(&args.handle, file, contents.clone(), mode)
                })?;
            }
        }
    }
    Ok(())
}
