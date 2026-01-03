use std::{collections::BTreeSet, sync::Arc};

use crate::shared::{profile::SeccompPolicy, syscalls, utility};
use anyhow::Result;
use caps::Capability;
use log::debug;
use spawn::Spawner;

pub fn install_filter(
    name: &str,
    instance: &str,
    policy: SeccompPolicy,
    binaries: Option<BTreeSet<String>>,
    main: &Spawner,
) -> Result<()> {
    if let Some((filter, fd, audit)) = syscalls::new(name, instance, policy, &binaries)? {
        main.seccomp_i(filter);

        if let Some(fd) = fd {
            main.fd_arg_i("--seccomp", fd)?;
        }

        if main.get_associate("monitor").is_none()
            && (policy == SeccompPolicy::Permissive || policy == SeccompPolicy::Notifying)
        {
            debug!("Spawning SECCOMP Monitor");
            let handle = Spawner::abs(utility("monitor"))
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
                .cap(Capability::CAP_AUDIT_READ)
                .mode(user::Mode::Original);

            if audit {
                handle.arg_i("--audit")?;
            }
            if log::log_enabled!(log::Level::Info) {
                handle.pass_env_i("RUST_LOG")?
            }
            main.associate(handle.spawn()?);
        }
    }
    Ok(())
}

pub fn setup(args: &Arc<super::Args>) -> Result<()> {
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

                install_filter(
                    &args.name,
                    args.instance.name(),
                    policy,
                    binaries,
                    &args.handle,
                )?;
            }
        }
    }
    Ok(())
}
