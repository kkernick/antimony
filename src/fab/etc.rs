use crate::aux::profile::Profile;
use std::path::Path;

pub fn fabricate(profile: &mut Profile, name: &str) {
    // Each is sent to the library fabricator, in case they contain anything,
    // and are then mounted directly.

    let etc = Path::new("/etc").join(name);
    if etc.exists() {
        profile
            .libraries
            .get_or_insert_default()
            .insert(etc.to_string_lossy().into_owned());
    }

    let share = Path::new("/usr").join("share").join(name);
    if share.exists() {
        profile
            .libraries
            .get_or_insert_default()
            .insert(share.to_string_lossy().into_owned());
    }

    let opt = Path::new("/opt").join(name);
    if opt.exists() {
        profile
            .libraries
            .get_or_insert_default()
            .insert(opt.to_string_lossy().into_owned());
    }
}
