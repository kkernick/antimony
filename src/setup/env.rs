use crate::shared::env::HOME;
use log::debug;
use rayon::prelude::*;

pub fn setup(args: &mut super::Args) {
    debug!("Setting up environment");
    if let Some(environment) = args.profile.environment.take() {
        environment
            .into_par_iter()
            .try_for_each(|(key, val)| {
                let mut val = val.replace(HOME.as_str(), "/home/antimony");
                if val.starts_with("$") {
                    val = std::env::var(&val[1..]).unwrap_or(val)
                }
                args.handle.args_i(["--setenv", &key, &val])
            })
            .ok();
    }
}
