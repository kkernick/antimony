//! Note the bulk of SECCOMP logic is in shared. This just attaches the Filter to a process.

use crate::shared::{Set, env::DATA_HOME, profile::seccomp::SeccompPolicy, syscalls, utility};
use anyhow::Result;
use caps::Capability;
use log::debug;
use spawn::Spawner;
use temp::Temp;

/// Install a filter onto a handle.
pub fn install_filter(
    name: &str,
    instance: &Temp,
    policy: SeccompPolicy,
    binaries: &Set<String>,
    main: &Spawner,
    monitor_parent: &Spawner,
    lockdown: bool,
) -> Result<()> {
    // Get our syscalls for this process.
    if let Some((filter, fd, audit)) = syscalls::new(name, instance, policy, binaries, lockdown)? {
        // Attach it.
        main.seccomp_i(filter);

        // Bwrap is confined under a broad policy that includes both it and the sandbox, to
        // which it then further confines the sandbox under a policy that only includes the sandbox.
        if let Some(fd) = fd {
            main.fd_arg_i("--seccomp", fd);
        }

        // If nobody has started a monitor yet, and we need one, attach it to the monitor_parent.
        if monitor_parent.get_associate("monitor").is_none()
            && (policy == SeccompPolicy::Permissive || policy == SeccompPolicy::Notifying)
        {
            if lockdown {
                return Err(anyhow::anyhow!(
                    "SECCOMP monitoring is disallowed in Lockdown. To use a SECCOMP policy, you must monitor without Lockdown, then use Enforcing."
                ));
            }
            debug!("Spawning SECCOMP Monitor");
            let handle = Spawner::abs(utility("monitor"))
                .name("monitor")
                .args([
                    "--instance",
                    &instance.full().to_string_lossy(),
                    "--profile",
                    name,
                    "--mode",
                    &format!("{policy}").to_lowercase(),
                ])
                .env_or("XDG_DATA_HOME", DATA_HOME.to_string_lossy())?
                .pass_env("DBUS_SESSION_BUS_ADDRESS")?
                .output(spawn::StreamMode::Log(log::Level::Info))
                .new_privileges(true)
                .cap(Capability::CAP_AUDIT_READ)
                .mode(user::Mode::Original);

            if audit {
                handle.arg_i("--audit");
            }
            if log::log_enabled!(log::Level::Info) {
                handle.pass_env_i("RUST_LOG")?;
            }
            monitor_parent.associate(handle.spawn()?);
        }
    }
    Ok(())
}

// Install the filter, if we need it.
pub fn setup(args: &super::Args) -> Result<()> {
    debug!("Setting up SECCOMP");
    // SECCOMP uses the elf binaries populated by the binary fabricator.
    let seccomp = args.profile.seccomp.unwrap_or_default();
    let lockdown = args.profile.lockdown.unwrap_or(false);

    match seccomp {
        SeccompPolicy::Disabled => {}
        policy => {
            if !args.run.dry {
                let binaries = &args.profile.binaries;
                install_filter(
                    &args.name,
                    args.instance,
                    policy,
                    binaries,
                    &args.handle,
                    &args.handle,
                    lockdown,
                )?;
            }
        }
    }
    Ok(())
}
