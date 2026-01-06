//! Note the bulk of SECCOMP logic is in shared. This just attaches the Filter to a process.

use crate::shared::{ISet, profile::seccomp::SeccompPolicy, syscalls, utility};
use anyhow::Result;
use caps::Capability;
use log::debug;
use spawn::Spawner;
use std::sync::Arc;

/// Install a filter onto a handle.
pub fn install_filter(
    name: &str,
    instance: &str,
    policy: SeccompPolicy,
    binaries: &ISet<String>,
    main: &Spawner,
    monitor_parent: &Spawner,
) -> Result<()> {
    // Get our syscalls for this process.
    if let Some((filter, fd, audit)) = syscalls::new(name, instance, policy, binaries)? {
        // Attach it.
        main.seccomp_i(filter);

        // Bwrap is confined under a broad policy that includes both it and the sandbox, to
        // which it then further confines the sandbox under a policy that only includes the sandbox.
        if let Some(fd) = fd {
            main.fd_arg_i("--seccomp", fd)?;
        }

        // If nobody has started a monitor yet, and we need one, attach it to the monitor_parent.
        if monitor_parent.get_associate("monitor").is_none()
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
            monitor_parent.associate(handle.spawn()?);
        }
    }
    Ok(())
}

// Install the filter, if we need it.
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
                let binaries = &args.profile.lock().binaries;
                install_filter(
                    &args.name,
                    args.instance.name(),
                    policy,
                    binaries,
                    &args.handle,
                    &args.handle,
                )?;
            }
        }
    }
    Ok(())
}
