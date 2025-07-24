use crate::aux::{profile::SeccompPolicy, syscalls};
use anyhow::Result;
use log::debug;
use spawn::Spawner;

pub fn setup(args: &mut super::Args) -> Result<()> {
    debug!("Setting up SECCOMP");

    // SECCOMP uses the elf binaries populated by the binary fabricator.
    match args.profile.seccomp.unwrap_or_default() {
        SeccompPolicy::Disabled => {}
        policy => {
            args.handle.seccomp_i(syscalls::new(
                &args.name,
                &args.instance,
                policy,
                &args.profile.binaries,
            )?);

            if policy == SeccompPolicy::Permissive && !args.args.dry {
                debug!("Spawning SECCOMP Monitor");
                let handle = Spawner::new("antimony-monitor")
                    .arg(&args.instance)?
                    .preserve_env(true)
                    .spawn()?;
                args.handle.associate(handle);
            }
        }
    }
    Ok(())
}
