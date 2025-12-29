use anyhow::Result;
use rayon::prelude::*;

pub fn fabricate(info: &super::FabInfo) -> Result<()> {
    if let Some(devices) = &info.profile.lock().devices {
        devices
            .par_iter()
            .try_for_each(|device| info.handle.args_i(["--dev-bind", device, device]))?;
    }
    Ok(())
}
