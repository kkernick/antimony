#![allow(clippy::missing_docs_in_private_items)]

use crate::{
    fab::files::localize,
    shared::{env::HOME, profile::files::FILE_MODES},
};
use anyhow::Result;
use log::debug;
use rayon::prelude::*;
use std::env;

#[inline]
pub fn setup(args: &mut super::Args) -> Result<()> {
    debug!("Setting up environment");
    args.profile.environment.par_iter().for_each(|(key, val)| {
        let mut val = val.replace(HOME.as_str(), "/home/antimony");
        // If we're passed an actual environment variable, resolve it.
        if val.starts_with('$') {
            val = env::var(&val[1..]).unwrap_or(val);
        }
        args.handle.args_i(["--setenv", key, &val]);
    });

    if let Some(files) = &args.profile.files {
        let runtime = &files.runtime;
        for mode in FILE_MODES {
            if let Some(files) = runtime.get(&mode) {
                for file in files {
                    localize(mode, file, false, &args.handle, true, &mut None)?;
                }
            }
        }
    }
    Ok(())
}
