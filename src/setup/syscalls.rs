use std::sync::Arc;

use crate::shared::{profile::SeccompPolicy, syscalls};
use anyhow::Result;
use log::debug;
use spawn::{Handle, Spawner};

pub fn setup(args: &Arc<super::Args>) -> Result<Option<Handle>> {
    debug!("Setting up SECCOMP");
    // SECCOMP uses the elf binaries populated by the binary fabricator.
    match args.profile.lock().seccomp.unwrap_or_default() {
        SeccompPolicy::Disabled => {}
        policy => {
            if !args.args.dry
                && let Some((filter, fd)) = syscalls::new(
                    &args.name,
                    args.instance.name(),
                    policy,
                    &args.profile.lock().binaries,
                    args.args.refresh,
                )?
            {
                args.handle.seccomp_i(filter);

                if let Some(fd) = fd {
                    args.handle.fd_arg_i("--seccomp", fd)?;
                }

                if policy == SeccompPolicy::Permissive || policy == SeccompPolicy::Notify {
                    debug!("Spawning SECCOMP Monitor");
                    #[rustfmt::skip]
                let handle = Spawner::abs("/usr/bin/antimony-monitor")
                    .args([
                        "--instance", args.instance.name(),
                        "--profile", &args.name,
                        "--mode", &format!("{policy:?}").to_lowercase()
                    ])?
                    .mode(user::Mode::Existing)
                    .preserve_env(true)
                    .spawn()?;
                    return Ok(Some(handle));
                }
            }
        }
    }
    Ok(None)
}
