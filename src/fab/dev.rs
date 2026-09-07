#![allow(clippy::missing_errors_doc)]

use anyhow::Result;

#[inline]
pub fn fabricate(info: &mut super::FabInfo) -> Result<()> {
    info.profile.devices.iter().for_each(|device| {
        info.handle.args_i(["--dev-bind-try", device, device]);
    });

    Ok(())
}
