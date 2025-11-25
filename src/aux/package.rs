use anyhow::Result;
use std::{
    fs::{self, File, copy, remove_file},
    io::{Read, Write},
    path::Path,
};
use walkdir::WalkDir;
use zip::write::SimpleFileOptions;

use crate::aux::env::PWD;

pub fn package(src: &Path, dst: &Path) -> Result<()> {
    let file = File::create(dst)?;
    let dir = WalkDir::new(src);
    let mut zip = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().unix_permissions(0o755);

    let prefix = Path::new(src);
    let mut buffer = Vec::new();
    for entry in dir.into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        let name = path.strip_prefix(prefix)?;
        let path_as_string = name.to_str().map(str::to_owned).unwrap_or_default();

        // Write file or directory explicitly
        // Some unzip tools unzip files with directory paths correctly, some do not!
        if path.is_file() {
            zip.start_file(path_as_string, options)?;
            let mut f = File::open(path)?;
            f.read_to_end(&mut buffer)?;
            zip.write_all(&buffer)?;
            buffer.clear();
        } else if !name.as_os_str().is_empty() {
            zip.add_directory(path_as_string, options)?;
        }
    }
    zip.finish()?;

    user::set(user::Mode::Real)?;
    copy(dst, PWD.join(dst.file_name().unwrap()))?;
    user::revert()?;
    remove_file(dst)?;
    Ok(())
}

pub fn extract(src: &Path, dst: &Path) -> Result<()> {
    if !dst.exists() {
        fs::create_dir_all(dst)?;
    }
    let file = File::open(src)?;
    let mut archive = zip::ZipArchive::new(file)?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let out = match file.enclosed_name() {
            Some(path) => dst.join(path),
            None => continue,
        };

        if file.is_dir() {
            fs::create_dir_all(&out)?;
        } else {
            if let Some(p) = out.parent()
                && !p.exists()
            {
                fs::create_dir_all(p)?;
            }
            let mut outfile = fs::File::create(&out)?;
            std::io::copy(&mut file, &mut outfile)?;
        }

        use std::os::unix::fs::PermissionsExt;

        if let Some(mode) = file.unix_mode() {
            fs::set_permissions(&out, fs::Permissions::from_mode(mode))?;
        }
    }
    Ok(())
}
