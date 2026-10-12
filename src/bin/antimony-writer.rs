//! A privilege helper for Antimony that can write hardlinks to the SOF with
//! Protected Hardlinks enforced by the system.
#![allow(unused_crate_dependencies)]

use antimony::fab::lib::add_sof;
use anyhow::{Result, anyhow};
use nix::unistd::Uid;
use rayon::prelude::*;
use std::{collections::VecDeque, env, fs::metadata, os::unix::fs::MetadataExt, path::PathBuf};

fn main() -> Result<()> {
    let antimony = metadata("/usr/bin/antimony")?.uid();
    if user::USER.real != Uid::from(antimony) {
        return Err(anyhow!("Must be called by Antimony!"));
    }

    let mut args: VecDeque<String> = env::args().collect();
    if args.pop_front().is_some()
        && let Some(sof) = args.pop_front()
        && let Some(cache) = args.pop_front()
    {
        let sof = PathBuf::from(sof);
        let cache = PathBuf::from(cache);
        args.par_iter().for_each(|lib| {
            if let Err(e) = add_sof(&sof, lib, &cache) {
                eprintln!("Failed to add {} to SOF: {e}", lib.as_str());
            }
        });
        Ok(())
    } else {
        Err(anyhow!(
            "Usage: antimony-writer [SOF] [Cache] [libraries...]"
        ))
    }
}
