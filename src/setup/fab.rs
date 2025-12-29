use crate::{
    fab::{self, FabInfo},
    shared::env::USER_NAME,
    timer,
};
use anyhow::Result;
use log::debug;
use std::{fs, sync::Arc};

pub fn setup(args: &Arc<super::Args>) -> Result<()> {
    // The fabricators are cached.
    let cmd_cache = args.sys_dir.join(USER_NAME.as_str()).join("cmd.cache");
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

    let info = FabInfo {
        profile: &args.profile,
        handle: &args.handle,
        name: &args.name,
        instance: args.instance,
        sys_dir: &args.sys_dir,
    };

    // Start caching.
    args.handle.cache_start()?;

    // Home must run before bin so that bin can populate files.
    timer!("::files", fab::files::fabricate(&info))?;
    timer!("::etc", fab::etc::fabricate(&info));
    timer!("::bin", fab::bin::fabricate(&info))?;
    timer!("::lib", fab::lib::fabricate(&info))?;
    timer!("::ns", fab::ns::fabricate(&info))?;
    timer!("::dev", fab::dev::fabricate(&info))?;

    timer!("::cache_write", args.handle.cache_write(&cmd_cache))?;
    Ok(())
}
