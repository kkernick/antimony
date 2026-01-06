use std::path::Path;

use log::trace;

pub fn fabricate(info: &super::FabInfo) {
    // Each is sent to the library fabricator, in case they contain anything,
    // and are then mounted directly.
    let etc = Path::new("/etc").join(info.name);
    if etc.exists() {
        trace!("Adding profile /etc");
        info.profile
            .lock()
            .libraries
            .insert(etc.to_string_lossy().into_owned());
    }

    let share = Path::new("/usr").join("share").join(info.name);
    if share.exists() {
        trace!("Adding profile /usr/share");
        info.profile
            .lock()
            .libraries
            .insert(share.to_string_lossy().into_owned());
    }

    let opt = Path::new("/opt").join(info.name);
    if opt.exists() {
        trace!("Adding profile /opt");
        info.profile
            .lock()
            .libraries
            .insert(opt.to_string_lossy().into_owned());
    }
}
