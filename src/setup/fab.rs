//! Set up the fabricators.
#![allow(clippy::missing_docs_in_private_items)]

use crate::{
    fab::{self, FabInfo},
    shared::{create_parent, env::USER_NAME},
    timer,
};
use anyhow::Result;
use std::path::{Path, PathBuf};
use user::as_effective;

#[inline]
pub fn cmd_cache(sys_dir: &Path) -> PathBuf {
    sys_dir.join(USER_NAME.as_str()).join("cmd.cache")
}

pub fn setup(args: &mut super::Args) -> Result<()> {
    // The fabricators are cached, but on disk.
    let cmd_cache = cmd_cache(&args.sys_dir);
    if cmd_cache.exists() && args.handle.cache_read(&cmd_cache).is_ok() {
        return Ok(());
    }

    as_effective!({ create_parent(cmd_cache.as_path()) })?;

    let mut info = FabInfo {
        profile: &mut args.profile,
        handle: &args.handle,
        name: &args.name,
        instance: args.instance,
        sys_dir: &args.sys_dir,
        policy: &mut args.policy,
    };

    // Start caching.
    args.handle.cache_start()?;

    // These can't be readily done in parallel, since
    // the heaviest ones (bin and lib) rely on each other.
    timer!("::fab::files", fab::files::fabricate(&mut info))?;
    timer!("::fab::bin", fab::bin::fabricate(&mut info))?;
    timer!("::fab::lib", fab::lib::fabricate(&mut info))?;
    timer!("::fab::ns", fab::ns::fabricate(&mut info))?;
    timer!("::fab::dev", fab::dev::fabricate(&mut info))?;

    as_effective!(args.handle.cache_write(&cmd_cache))?;

    Ok(())
}
