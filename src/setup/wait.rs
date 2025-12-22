use anyhow::Result;
use log::debug;

use crate::debug_timer;

pub fn setup(args: &mut super::Args) -> Result<()> {
    debug_timer!("::inotify", {
        if !args.watches.is_empty() && !args.args.dry {
            debug!("Waiting for inotify");
            let mut buffer = [0; 1024];
            while !args.watches.is_empty() {
                let events = args.inotify.read_events_blocking(&mut buffer)?;
                for event in events {
                    if args.watches.contains(&event.wd) {
                        if let Some(path) = event.name {
                            debug!("Finished Notify Event: {path:?}");
                        }
                        args.watches.remove(&event.wd);
                    }
                }
            }
        }
    });
    Ok(())
}
