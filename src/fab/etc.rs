use std::path::Path;

pub fn fabricate(info: &super::FabInfo) {
    // Each is sent to the library fabricator, in case they contain anything,
    // and are then mounted directly.

    let etc = Path::new("/etc").join(info.name);
    if etc.exists() {
        info.profile
            .lock()
            .libraries
            .insert(etc.to_string_lossy().into_owned());
    }

    let share = Path::new("/usr").join("share").join(info.name);
    if share.exists() {
        info.profile
            .lock()
            .libraries
            .insert(share.to_string_lossy().into_owned());
    }

    let opt = Path::new("/opt").join(info.name);
    if opt.exists() {
        info.profile
            .lock()
            .libraries
            .insert(opt.to_string_lossy().into_owned());
    }
}
