#![allow(clippy::missing_docs_in_private_items)]
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
    if cmd_cache.exists() {
        debug!("Using cached fabricators");
        if args.handle.cache_read(&cmd_cache).is_ok() {
            return Ok(());
        }
        debug!("Corrupted cache. Rebuilding.");
    }

    if let Some(parent) = cmd_cache.parent()
        && !parent.exists()
    {
        fs::create_dir_all(parent)?;
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
    timer!("::fab::files", fab::files::fabricate(&info))?;
    timer!("::fab::bin", fab::bin::fabricate(&mut info))?;
    timer!("::fab::lib", fab::lib::fabricate(&mut info))?;
    timer!("::fab::ns", fab::ns::fabricate(&mut info))?;
    timer!("::fab::dev", fab::dev::fabricate(&info))?;

    timer!("::fab::cache_write", args.handle.cache_write(&cmd_cache))?;
    Ok(())
}
