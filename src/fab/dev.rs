#![allow(clippy::missing_errors_doc)]

use anyhow::Result;

#[inline]
pub fn fabricate(info: &mut super::FabInfo) -> Result<()> {
    if let Some(devices) = &info.profile.devices {
        for device in devices {
            info.handle.args_i(["--dev-bind-try", device, device]);
        }
    }

    Ok(())
}
