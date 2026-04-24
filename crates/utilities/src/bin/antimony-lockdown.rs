//! This application serves as a SetUID hand-off when Antimony is running in
//! lock-down mode. In Lockdown, we run underneath a dedicated, isolated user,
//! so Antimony needs to transition from $USER/antimony to lockdown/antimony.
//! This is achieved by having a SetUID owned by the lockdown that does nothing
//! but ingest a bubblewrap command and execute.

use anyhow::{Result, anyhow};
use spawn::Spawner;
use std::{
    env,
    fs::{self, metadata},
    os::{
        fd::{FromRawFd, OwnedFd},
        unix::fs::MetadataExt,
    },
    path::PathBuf,
};
use user::{USER, as_effective};

fn main() -> Result<()> {
    let self_path = PathBuf::from("/usr/share/antimony/utilities/antimony-lockdown");

    let real = USER.real.as_raw();
    let effective = USER.effective.as_raw();

    let antimony = metadata("/usr/bin/antimony")?.uid();
    let lockdown = self_path.metadata()?.uid();

    // We don't want other users running as Lockdown, since they can access the Lockdown
    // Home store.
    if real != antimony {
        return Err(anyhow!(
            "Only antimony is allowed to run this utility! {real} vs {antimony}"
        ));
    }

    // We only want to be running from the system installation to avoid misconfiguration.
    if fs::read_link("/proc/self/exe")? != self_path {
        return Err(anyhow!(
            "Lockdown must be run from the system installation!"
        ));
    }

    // We want to ensure we are actually running as the lockdown-user, otherwise
    // this doesn't confer any security benefit.
    if effective != lockdown {
        return Err(anyhow!(
            "Lockdown user is not configured correctly! {effective} vs {lockdown}"
        ));
    }

    // The Lockdown user has to make the HOME itself.
    if let Ok(home) = env::var("LOCKDOWN_HOME") {
        let path = PathBuf::from(format!("/usr/share/antimony/lockdown/{home}"));
        if !path.exists() {
            as_effective!(fs::create_dir_all(path))??;
        }
    }

    // Digest the arguments and pass them to bubblewrap.
    let arguments: Vec<_> = env::args().collect();
    let handle = Spawner::abs("/usr/bin/bwrap")
        .mode(user::Mode::Effective)
        .args(&arguments[1..]);

    // Pass all FDs Antimony tells us to.
    let fds: Vec<OwnedFd> = env::var("FDS")
        .unwrap_or_default()
        .split(",")
        .filter_map(|fd: &str| -> Option<i32> { fd.parse::<i32>().ok() })
        .map(|fd| unsafe { OwnedFd::from_raw_fd(fd) })
        .collect();

    // Launch
    handle.fds(fds).spawn()?.wait()?;

    Ok(())
}
