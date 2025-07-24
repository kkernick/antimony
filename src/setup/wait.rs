use anyhow::Result;
use log::debug;

pub fn setup(args: &mut super::Args) -> Result<()> {
    if !args.watches.is_empty() && !args.args.dry {
        debug!("Waiting for inotify");
        let mut buffer = [0; 1024];
        while !args.watches.is_empty() {
            let events = args.inotify.read_events_blocking(&mut buffer)?;
            for event in events {
                if args.watches.contains(&event.wd) {
                    args.watches.remove(&event.wd);
                }
            }
        }
    }
    Ok(())
}
