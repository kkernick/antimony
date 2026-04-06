//! Setup the fabricators.

use crate::{
    fab::{self, FabInfo},
    shared::env::USER_NAME,
    timer,
};
use anyhow::Result;
use log::debug;
use std::{
    fs,
    path::{Path, PathBuf},
};

#[inline]
pub fn cmd_cache(sys_dir: &Path) -> PathBuf {
    sys_dir.join(USER_NAME.as_str()).join("cmd.cache")
}

pub fn setup(args: &mut super::Args) -> Result<()> {
    // The fabricators are cached, but on disk.
    let cmd_cache = cmd_cache(&args.sys_dir);
    if let Some(parent) = cmd_cache.parent()
        && !parent.exists()
    {
        fs::create_dir_all(parent)?;
    }

    if cmd_cache.exists() {
        debug!("Using cached fabricators");
        if args.handle.cache_read(&cmd_cache).is_ok() {
            return Ok(());
        }
        debug!("Corrupted cache. Rebuilding.");
    }

    debug!("Fabricating sandbox");

    let mut info = FabInfo {
        profile: &mut args.profile,
        handle: &args.handle,
        name: &args.name,
        instance: args.instance,
        sys_dir: &args.sys_dir,
    };

    // Start caching.
    args.handle.cache_start()?;

    // These can't be readily done in parallel, since
    // the heaviest ones (bin and lib) rely on each other.
    timer!("::files", fab::files::fabricate(&info))?;
    timer!("::bin", fab::bin::fabricate(&mut info))?;
    timer!("::lib", fab::lib::fabricate(&mut info))?;
    timer!("::ns", fab::ns::fabricate(&mut info))?;
    timer!("::dev", fab::dev::fabricate(&info))?;

    timer!("::cache_write", args.handle.cache_write(&cmd_cache))?;
    Ok(())
}
