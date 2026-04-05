use crate::fab::{
    get_dir,
    lib::{self, ROOTS},
};
use log::info;
use rayon::prelude::*;
use std::path::Path;

pub fn resolve(path: &Path) {
    if path.exists() {
        info!("Adding application folder: {}", path.display());
        let str = path.to_string_lossy().into_owned();
        if let Ok(libraries) = get_dir(&str) {
            libraries.into_iter().for_each(|lib| {
                let _ = lib::FILES.insert(lib);
            });
        }
        lib::DIRS.insert(str);
    }
}

pub fn fabricate(info: &super::FabInfo) {
    // Each is sent to the library fabricator, in case they contain anything,
    // and are then mounted directly.
    [
        Path::new("/etc"),
        Path::new("/usr/share"),
        Path::new("/opt"),
    ]
    .into_par_iter()
    .for_each(|path| resolve(&path.join(info.name)));

    ROOTS
        .par_iter()
        .for_each(|lib_root| resolve(Path::new(&format!("{}/{}", lib_root.as_str(), info.name))));
}
