use anyhow::Result;
use rayon::prelude::*;

#[inline]
pub fn fabricate(info: &super::FabInfo) -> Result<()> {
    info.profile
        .lock()
        .devices
        .par_iter()
        .try_for_each(|device| info.handle.args_i(["--dev-bind", device, device]))?;

    Ok(())
}
