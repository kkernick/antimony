use crate::shared::env::HOME;
use log::debug;
use rayon::prelude::*;
use std::sync::Arc;

pub fn setup(args: &Arc<super::Args>) {
    debug!("Setting up environment");
    args.profile
        .lock()
        .environment
        .par_iter()
        .try_for_each(|(key, val)| {
            let mut val = val.replace(HOME.as_str(), "/home/antimony");
            // If we're passed an actual environment variable, resolve it.
            if val.starts_with("$") {
                val = std::env::var(&val[1..]).unwrap_or(val)
            }
            args.handle.args_i(["--setenv", key, &val])
        })
        .ok();
}
