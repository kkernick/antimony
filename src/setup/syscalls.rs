use std::{collections::BTreeSet, sync::Arc};

use crate::shared::{env::AT_HOME, profile::SeccompPolicy, syscalls};
use anyhow::Result;
use log::debug;
use spawn::{Handle, Spawner};

pub fn install_filter(
    name: &str,
    instance: &str,
    policy: SeccompPolicy,
    binaries: Option<BTreeSet<String>>,
    handle: &Spawner,
) -> Result<Option<Handle>> {
    if let Some((filter, fd, audit)) = syscalls::new(name, instance, policy, &binaries)? {
        handle.seccomp_i(filter);

        if let Some(fd) = fd {
            handle.fd_arg_i("--seccomp", fd)?;
        }

        if policy == SeccompPolicy::Permissive || policy == SeccompPolicy::Notifying {
            debug!("Spawning SECCOMP Monitor");
            let mut handle = Spawner::abs(
                AT_HOME
                    .join("utilities")
                    .join("antimony-monitor")
                    .to_string_lossy(),
            )
            .name("monitor")
            .args([
                "--instance",
                instance,
                "--profile",
                name,
                "--mode",
                &format!("{policy}").to_lowercase(),
            ])?
            .pass_env("XDG_DATA_HOME")?
            .pass_env("XDG_RUNTIME_DIR")?
            .pass_env("DBUS_SESSION_BUS_ADDRESS")?
            .output(spawn::StreamMode::Log(log::Level::Info))
            .new_privileges(true)
            .mode(user::Mode::Original);

            if audit {
                handle.arg_i("--audit")?;
            }
            if log::log_enabled!(log::Level::Info) {
                handle.pass_env_i("RUST_LOG")?
            }
            return Ok(Some(handle.spawn()?));
        }
    }
    Ok(None)
}

pub fn setup(args: &Arc<super::Args>) -> Result<Option<Handle>> {
    debug!("Setting up SECCOMP");
    // SECCOMP uses the elf binaries populated by the binary fabricator.
    let seccomp = {
        let lock = args.profile.lock();
        lock.seccomp.unwrap_or_default()
    };

    match seccomp {
        SeccompPolicy::Disabled => {}
        policy => {
            if !args.args.dry {
                let binaries = {
                    let mut lock = args.profile.lock();
                    lock.binaries.take()
                };

                return install_filter(
                    &args.name,
                    args.instance.name(),
                    policy,
                    binaries,
                    &args.handle,
                );
            }
        }
    }
    Ok(None)
}
