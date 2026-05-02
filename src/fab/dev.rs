#![allow(clippy::missing_errors_doc)]

use anyhow::Result;
use rayon::prelude::*;

#[inline]
pub fn fabricate(info: &super::FabInfo) -> Result<()> {
    info.profile
        .devices
        .par_iter()
        .for_each(|device| info.handle.args_i(["--dev-bind", device, device]));

    Ok(())
}
