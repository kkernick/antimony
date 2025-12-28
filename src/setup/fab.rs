use crate::{debug_timer, fab, shared::env::USER_NAME};
use anyhow::Result;
use log::debug;
use std::fs;

pub fn setup(args: &mut super::Args) -> Result<()> {
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

    // Start caching.
    args.handle.cache_start()?;

    // Home must run before bin so that bin can populate files.
    debug_timer!(
        "::files",
        fab::files::fabricate(&mut args.profile, &args.handle)
    )?;

    debug_timer!("::etc", fab::etc::fabricate(&mut args.profile, &args.name));

    // Bin must run before lib so that bin can populate libraries
    debug_timer!(
        "::bin",
        fab::bin::fabricate(&mut args.profile, &args.instance, &args.name, &args.handle)
    )?;

    debug_timer!(
        "::lib",
        fab::lib::fabricate(&mut args.profile, &args.name, &args.sys_dir, &args.handle)
    )?;

    debug_timer!("::ns", fab::ns::fabricate(&mut args.profile, &args.handle))?;

    debug_timer!(
        "::dev",
        fab::dev::fabricate(&mut args.profile, &args.handle)
    )?;

    debug_timer!("::cache_write", args.handle.cache_write(&cmd_cache))?;
    Ok(())
}
