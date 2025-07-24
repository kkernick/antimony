use crate::fab;
use anyhow::Result;
use log::debug;

pub fn setup(args: &mut super::Args) -> Result<()> {
    // The fabricators are cached.
    let cmd_cache = args.sys_dir.join("cmd.cache");

    if cmd_cache.exists() {
        debug!("Using cached fabricators");
        if args.handle.cache_read(&cmd_cache).is_ok() {
            return Ok(());
        }
        debug!("Corrupted cache. Rebuilding.");
    }

    debug!("Fabricating sandbox");
    std::fs::create_dir_all(&args.sys_dir)?;

    // Start caching.
    args.handle.cache_start()?;

    // Home must run before bin so that bin can populate files.
    debug!("Fabricating /home");
    fab::home::fabricate(&mut args.profile, &args.handle)?;

    debug!("Fabricating /etc");
    fab::etc::fabricate(&mut args.profile, &args.name);

    // Bin must run before lib so that bin can populate libraries
    debug!("Fabricating binaries");
    fab::bin::fabricate(&mut args.profile, &args.name, &args.handle)?;

    debug!("Fabricating /lib");
    fab::lib::fabricate(&mut args.profile, &args.name, &args.sys_dir, &args.handle)?;

    debug!("Fabricating namespaces");
    fab::ns::fabricate(&mut args.profile, &args.handle)?;

    debug!("Fabricating /dev");
    fab::dev::fabricate(&mut args.profile, &args.handle)?;

    debug!("Writing fabricator cache: {}", cmd_cache.display());
    args.handle.cache_write(&cmd_cache)?;
    Ok(())
}
