use crate::shared::{profile::SeccompPolicy, syscalls};
use anyhow::Result;
use log::debug;
use spawn::Spawner;

pub fn setup(args: &mut super::Args) -> Result<()> {
    debug!("Setting up SECCOMP");
    // SECCOMP uses the elf binaries populated by the binary fabricator.
    match args.profile.seccomp.unwrap_or_default() {
        SeccompPolicy::Disabled => {}
        policy => {
            if !args.args.dry {
                let (filter, fd) =
                    syscalls::new(&args.name, &args.instance, policy, &args.profile.binaries)?;

                args.handle.seccomp_i(filter);

                if let Some(fd) = fd {
                    args.handle.fd_arg_i("--seccomp", fd)?;
                }

                if policy == SeccompPolicy::Permissive || policy == SeccompPolicy::Notify {
                    debug!("Spawning SECCOMP Monitor");
                    #[rustfmt::skip]
                let handle = Spawner::new("/usr/bin/antimony-monitor")
                    .args([
                        "--instance", args.instance.as_str(),
                        "--profile", &args.name,
                        "--mode", &format!("{policy:?}").to_lowercase()
                    ])?
                    .mode(user::Mode::Existing)
                    .preserve_env(true)
                    .spawn()?;
                    args.handle.associate(handle);
                }
            }
        }
    }
    Ok(())
}
