use crate::aux::env::HOME;
use log::debug;
use rayon::prelude::*;

pub fn setup(args: &mut super::Args) {
    debug!("Setting up environment");
    if let Some(environment) = args.profile.environment.take() {
        environment
            .into_par_iter()
            .try_for_each(|(key, mut val)| {
                val = val.replace(HOME.as_str(), "/home/antimony");
                args.handle.args_i(["--setenv", &key, &val])
            })
            .ok();
    }
}
