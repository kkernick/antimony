use crate::shared::env::HOME;
use log::debug;
use rayon::prelude::*;

#[inline]
pub fn setup(args: &super::Args) {
    debug!("Setting up environment");
    args.profile.environment.par_iter().for_each(|(key, val)| {
        let mut val = val.replace(HOME.as_str(), "/home/antimony");
        // If we're passed an actual environment variable, resolve it.
        if val.starts_with("$") {
            val = std::env::var(&val[1..]).unwrap_or(val)
        }
        args.handle.args_i(["--setenv", key, &val])
    })
}
