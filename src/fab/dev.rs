use crate::aux::profile::Profile;
use anyhow::Result;
use rayon::prelude::*;
use spawn::Spawner;

pub fn fabricate(profile: &mut Profile, handle: &Spawner) -> Result<()> {
    if let Some(devices) = profile.devices.take() {
        devices
            .into_par_iter()
            .try_for_each(|device| handle.args_i(["--dev-bind", &device, &device]))?;
    }
    Ok(())
}
