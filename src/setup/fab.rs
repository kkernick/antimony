use crate::{
    fab::{self, lib::LIB_ROOTS},
    shared::env::USER_NAME,
};
use anyhow::Result;
use log::debug;
use once_cell::sync::Lazy;
use std::{fs, time::Instant};

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

    // Get the lib roots as soon as possible.
    rayon::spawn(|| {
        Lazy::force(&LIB_ROOTS);
    });

    debug!("Fabricating sandbox");
    let mut timer = Instant::now();
    fs::create_dir_all(&args.sys_dir)?;

    // Start caching.
    args.handle.cache_start()?;

    // Home must run before bin so that bin can populate files.
    debug!("Fabricating Files");
    fab::files::fabricate(&mut args.profile, &args.handle)?;
    debug!("Fabricated Files in: {}ms", timer.elapsed().as_millis());

    debug!("Fabricating /etc");
    timer = Instant::now();
    fab::etc::fabricate(&mut args.profile, &args.name);
    debug!("Fabricated /etc in: {}ms", timer.elapsed().as_millis());

    // Bin must run before lib so that bin can populate libraries
    debug!("Fabricating binaries");
    timer = Instant::now();
    fab::bin::fabricate(&mut args.profile, &args.name, &args.handle)?;
    debug!("Fabricated /bin in: {}ms", timer.elapsed().as_millis());

    debug!("Fabricating /lib");
    timer = Instant::now();
    fab::lib::fabricate(&mut args.profile, &args.name, &args.sys_dir, &args.handle)?;
    debug!("Fabricated /lib in: {}ms", timer.elapsed().as_millis());

    debug!("Fabricating namespaces");
    timer = Instant::now();
    fab::ns::fabricate(&mut args.profile, &args.handle)?;
    debug!(
        "Fabricated namespaces in: {}ms",
        timer.elapsed().as_millis()
    );

    debug!("Fabricating /dev");
    timer = Instant::now();
    fab::dev::fabricate(&mut args.profile, &args.handle)?;
    debug!("Fabricated /dev in: {}ms", timer.elapsed().as_millis());

    debug!("Writing fabricator cache: {}", cmd_cache.display());
    args.handle.cache_write(&cmd_cache)?;
    Ok(())
}
