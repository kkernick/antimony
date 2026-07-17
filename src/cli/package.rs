//! Export user-profiles

use crate::{
    cli::{run, run_vec},
    fab::{get_libraries, lib::ROOTS},
    setup::setup,
    shared::{
        Map,
        env::PWD,
        find::{DirType, recursive_crawl},
        package::{PACKAGE_MARKER, Package},
        profile::Profile,
        store,
    },
};
use anyhow::Result;
use bilrost::Message;
use clap::ValueHint;
use log::info;
use std::{
    borrow::Cow,
    env,
    fs::{self, File},
    io::{Read, Seek, SeekFrom, Write},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

#[derive(clap::Args)]
pub struct Args {
    /// The name of the profile/feature to export. If absent, export all user-profiles/features.
    #[arg(value_hint = ValueHint::CommandName)]
    profile: String,

    /// Where to export to. Defaults to current directory
    #[arg(short, long, value_hint = ValueHint::DirPath)]
    dest: Option<String>,

    /// An optional version string for the package
    #[arg(short, long)]
    version: Option<String>,

    /// Run arguments
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub passthrough: Option<Vec<String>>,
}
impl super::Run for Args {
    #[allow(clippy::unwrap_used)]
    #[allow(clippy::too_many_lines)]
    fn run(self) -> Result<()> {
        store::CACHE.lock().replace(false);
        let name = if let Some(version) = self.version {
            format!("{}-{}.sb", self.profile, version)
        } else {
            format!("{}.sb", self.profile)
        };

        let dest = self
            .dest
            .map_or_else(|| PWD.clone(), PathBuf::from)
            .join(name);
        let (profile, _) = Profile::new(&self.profile, None, None, false)?;
        let exe_path = env::current_exe()?;
        let mut binary_file = File::open(&exe_path)?;

        let mut binary_content = Vec::new();
        binary_file.read_to_end(&mut binary_content)?;

        // 2. Prepare the data
        let mut package = Package {
            name: self.profile.clone(),
            profile,
            ..Default::default()
        };

        package.add_system("bwrap", "bwrap")?;
        package.add_system("bash", "bash")?;
        package.add_system("ldd", "ldd")?;
        package.add_system("xdg-dbus-proxy", "xdg-dbus-proxy")?;

        let integration_bundle = |id: &str, package: &mut Package| -> Result<()> {
            recursive_crawl("/usr/share/applications", None)?
                .remove(&DirType::File)
                .unwrap_or_default()
                .into_iter()
                .filter(|p| p.contains(id))
                .try_for_each(|p| package.add_misc(&p, &p))?;

            recursive_crawl("/usr/share/icons", None)?
                .remove(&DirType::File)
                .unwrap_or_default()
                .into_iter()
                .filter(|p| p.contains(id))
                .try_for_each(|p| package.add_misc(&p, &p))?;
            Ok(())
        };

        let id = package
            .profile
            .id
            .as_ref()
            .map_or_else(|| self.profile.clone(), Clone::clone);
        integration_bundle(&id, &mut package)?;
        for (name, config) in package.profile.configuration.clone() {
            let c_id = config
                .id
                .as_ref()
                .map_or_else(|| self.profile.clone(), Clone::clone);
            if config.id(&name) != id {
                integration_bundle(&c_id, &mut package)?;
            }
        }

        let mut args = self
            .passthrough
            .map_or_else(run::Args::default, |passthrough| {
                run_vec(&self.profile, passthrough)
            });
        args.dry = true;
        args.refresh = true;

        let mut package = setup(
            Cow::Owned(self.profile),
            &mut args,
            false,
            Some((package, false)),
        )?
        .package
        .unwrap()
        .0;

        let roots = &mut package.profile.libraries.get_or_insert_default().roots;
        roots.extend(ROOTS.iter().map(|r| String::from(r.as_ref())));

        let mut depend = Map::default();
        let mut collect = |bin| -> Result<()> {
            for library in get_libraries(which::which(bin)?)? {
                if !package.system_libraries.contains_key(library.as_str()) {
                    let mut dest = library.as_str();
                    for root in ROOTS.iter() {
                        dest = dest.strip_prefix(root.as_ref()).unwrap_or(dest);
                    }
                    dest = dest.strip_prefix('/').unwrap_or(dest);
                    Package::add_path(&library, dest, &mut depend)?;
                }
            }
            Ok(())
        };

        let antimony = env::current_exe()?.to_string_lossy().into_owned();
        collect(&antimony)?;
        for bin in package.system_binaries.keys() {
            collect(bin)?;
        }
        package.system_libraries = depend;

        info!("Packing...");
        let bytes = zstd::encode_all(Package::encode_to_vec(&package).as_slice(), 3)?;

        let mut out_file = File::create(&dest)?;

        // Write original binary
        out_file.write_all(&binary_content)?;

        // Write Header: Marker + Length
        out_file.write_all(&PACKAGE_MARKER)?;

        // Write Payload
        out_file.write_all(&bytes)?;

        out_file.seek(SeekFrom::Start(9))?;
        out_file.write_all(&PACKAGE_MARKER)?;

        out_file.sync_all()?;
        let metadata = fs::metadata(&dest)?;
        let mut perms = metadata.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(dest, perms)?;

        Ok(())
    }
}
