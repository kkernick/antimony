//! Convert the profile into a self-contained package.
use crate::{
    fab::{
        bin,
        lib::{self, get_sof_path},
    },
    shared::{
        env::{CACHE_DIR, USER_NAME},
        package,
        profile::{FileMode, Profile},
    },
};
use anyhow::Result;
use copy_dir::copy_dir;
use log::debug;
use rayon::prelude::*;
use spawn::Spawner;
use std::{
    fs::{self, copy},
    os::unix::fs::symlink,
    path::{Path, PathBuf},
};
use strum::IntoEnumIterator;

#[derive(clap::Args, Debug)]
pub struct Args {
    /// The name of the profile
    pub profile: String,

    /// Use a configuration within the profile.
    #[arg(short, long)]
    pub config: Option<String>,
}
impl super::Run for Args {
    fn run(self) -> Result<()> {
        let name = &self.profile;

        let profile_path = Profile::path(name)?;
        let mut profile = Profile::new(name)?;

        let sys_dir = CACHE_DIR.join(profile.hash_str());
        let user_cache = sys_dir.join(USER_NAME.as_str());

        profile = profile.integrate(name, &user_cache)?;

        let package_dir = sys_dir.join("package");
        if package_dir.exists() {
            fs::remove_dir_all(&package_dir)?;
        }

        fs::create_dir_all(&package_dir)?;
        fs::copy(profile_path, package_dir.join("profile.toml"))?;

        debug!("Creating package bin");
        fs::create_dir_all(package_dir.join("usr").join("bin"))?;

        profile
            .binaries
            .get_or_insert_default()
            .extend(["sh".to_string(), "strace".to_string()]);

        debug!("Finding binaries");
        let parsed = bin::collect(&mut profile, name)?;
        profile.binaries = Some(parsed.elf.iter().map(|e| e.clone()).collect());

        debug!("Adding binaries: {parsed:?}");
        parsed
            .elf
            .into_par_iter()
            .try_for_each(|elf| -> Result<()> {
                copy(&elf, package_dir.join(&elf[1..]))?;
                Ok(())
            })?;

        parsed
            .scripts
            .into_par_iter()
            .try_for_each(|script| -> Result<()> {
                copy(&script, package_dir.join(&script[1..]))?;
                Ok(())
            })?;

        parsed
            .files
            .into_par_iter()
            .try_for_each(|file| -> Result<()> {
                copy(&file, package_dir.join(&file[1..]))?;
                Ok(())
            })?;

        parsed
            .symlinks
            .into_iter()
            .try_for_each(|(link, dest)| -> Result<()> {
                let package_dest = package_dir.join(&dest[1..]);
                let package_link = package_dir.join(&link[1..]);
                copy(&dest, &package_dest)?;
                symlink(package_dest, package_link)?;
                Ok(())
            })?;

        parsed
            .localized
            .into_par_iter()
            .try_for_each(|(src, dst)| -> Result<()> {
                copy(&src, package_dir.join(&dst[1..]))?;
                Ok(())
            })?;

        profile
            .libraries
            .get_or_insert_default()
            .extend(parsed.directories);

        debug!("Finding libraries");
        lib::fabricate(&mut profile, name, &package_dir, &Spawner::new(""))?;

        debug!("Adding directories");
        if let Some(directories) = profile.libraries {
            directories
                .into_par_iter()
                .try_for_each(|dir| -> Result<()> {
                    debug!("Adding {dir}");
                    let path = get_sof_path(&package_dir.join("sof"), &dir);
                    fs::remove_dir(&path)?;
                    copy_dir(&dir, path)?;
                    Ok(())
                })?;
        }

        debug!("Populating root");
        let root = package_dir.join("root");
        fs::create_dir_all(&root)?;

        let etc = Path::new("/etc").join(name);
        if etc.exists() {
            debug!("Adding /etc");
            fs::create_dir_all(root.join("etc"))?;
            copy_dir(&etc, root.join(format!("etc/{name}")))?;
        }

        let share = Path::new("/usr/share").join(name);
        if share.exists() {
            debug!("Adding /usr/share");
            fs::create_dir_all(root.join("usr").join("share"))?;
            copy_dir(&share, root.join(format!("usr/share/{name}")))?;
        }

        let opt = Path::new("/opt").join(name);
        if opt.exists() {
            debug!("Adding /opt");
            fs::create_dir_all(root.join("opt"))?;
            copy_dir(&opt, root.join(format!("opt/{name}")))?;
        }

        if let Some(mut files) = profile.files.take()
            && let Some(mut system) = files.resources.take()
        {
            for mode in FileMode::iter() {
                if let Some(files) = system.remove(&mode) {
                    files.into_iter().try_for_each(|file| -> Result<()> {
                        debug!("Adding {file}");
                        let path = PathBuf::from(&file);
                        let dest = root.join(&file[1..]);

                        if let Some(parent) = dest.parent() {
                            fs::create_dir_all(parent)?;
                        }

                        if path.is_file() {
                            copy(path, dest)?;
                        } else if path.is_dir() {
                            copy_dir(path, dest)?;
                        }
                        Ok(())
                    })?
                }
            }
        }

        let p_path = sys_dir.join(format!("{}.sb", self.profile));
        debug!("Packaging");
        package::package(&package_dir, &p_path)?;
        fs::remove_dir_all(&package_dir)?;
        Ok(())
    }
}
