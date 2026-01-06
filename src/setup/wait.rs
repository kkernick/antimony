use anyhow::{Result, anyhow};
use dashmap::DashSet;
use inotify::{Inotify, WatchDescriptor};
use log::debug;
use spawn::Spawner;

use crate::timer;

/// Wait for everything to be ready.
pub fn setup(
    watches: DashSet<WatchDescriptor>,
    mut inotify: Inotify,
    handle: &mut Spawner,
    dry: bool,
) -> Result<()> {
    
    // Ensure the proxy didn't die.
    if let Some(mut proxy) = handle.get_associate("proxy")
        && proxy.alive()?.is_none()
    {
        return Err(anyhow!("Proxy died!"));
    }

    // Wait for the bus to be available.
    timer!("::inotify", {
        if !watches.is_empty() && !dry {
            debug!("Waiting for inotify");
            let mut buffer = [0; 1024];
            while !watches.is_empty() {
                let events = inotify.read_events_blocking(&mut buffer)?;
                for event in events {
                    if watches.contains(&event.wd) {
                        if let Some(path) = event.name {
                            debug!("Finished Notify Event: {}", path.display());
                        }
                        watches.remove(&event.wd);
                    }
                }
            }
        }
    });
    Ok(())
}
