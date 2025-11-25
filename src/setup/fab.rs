use std::fs;

use crate::{
    aux::env::{OVERLAY, USER_NAME},
    fab,
};
use anyhow::{Result, anyhow};
use log::debug;

pub fn setup(args: &mut super::Args) -> Result<()> {
    // The fabricators are cached.
    let cmd_cache = args.sys_dir.join(USER_NAME.as_str()).join("cmd.cache");
    if let Some(parent) = cmd_cache.parent()
        && !parent.exists()
    {
        fs::create_dir_all(parent)?;
    }

    if cmd_cache.exists() {
        debug!("Using cached fabricators");
        if args.handle.cache_read(&cmd_cache).is_ok() {
            return Ok(());
        }
        debug!("Corrupted cache. Rebuilding.");
    }

    debug!("Fabricating sandbox");
    std::fs::create_dir_all(&args.sys_dir)?;

    // Start caching.
    args.handle.cache_start()?;

    // Home must run before bin so that bin can populate files.
    debug!("Fabricating Files");
    fab::files::fabricate(&mut args.profile, &args.handle, args.package.is_some())?;

    if let Some(package_path) = &args.package {
        let root = package_path.join("root");
        let bin = package_path.join("usr").join("bin");
        let sof = package_path.join("sof");

        let sof_string = sof.to_string_lossy();
        let bin_string = bin.to_string_lossy();

        let work = args.sys_dir.join("work");
        fs::create_dir_all(&work)?;
        let work_str = work.to_string_lossy();

        if !*OVERLAY {
            return Err(anyhow!("Overlay support is required for packages!"));
        }

        #[rustfmt::skip]
        args.handle.args_i([
            "--ro-bind-try", &format!("{sof_string}/lib"), "/usr/lib",
            "--ro-bind-try", &format!("{sof_string}/lib64"), "/usr/lib64",
            "--overlay-src", &work_str, "--overlay-src", &bin_string, "--ro-overlay", "/usr/bin",
            "--symlink", "/usr/lib", "/lib",
            "--symlink", "/usr/lib64", "/lib64",
            "--symlink", "/usr/bin", "/bin",
            "--symlink", "/usr/sbin", "/sbin"
        ])?;

        for entry in fs::read_dir(root)?.filter_map(|e| e.ok()) {
            let path = entry.path();
            let name = path.file_name().unwrap().to_string_lossy();

            if name == "usr" {
                for entry in fs::read_dir(path)?.filter_map(|e| e.ok()) {
                    let path = entry.path();
                    let name = path.file_name().unwrap().to_string_lossy();

                    if name == "share" {
                        for entry in fs::read_dir(path)?.filter_map(|e| e.ok()) {
                            let path = entry.path();
                            let name = path.file_name().unwrap().to_string_lossy();
                            let string = path.to_string_lossy();

                            #[rustfmt::skip]
                            args.handle.args_i([
                                "--ro-bind",  &string, &format!("/usr/share/{name}")
                            ])?;
                        }
                    } else {
                        let path = entry.path();
                        let name = path.file_name().unwrap().to_string_lossy();
                        let string = path.to_string_lossy();

                        #[rustfmt::skip]
                        args.handle.args_i([
                            "--ro-bind",  &string, &format!("/usr/{name}"),
                        ])?;
                    }
                }
            } else {
                let string = path.to_string_lossy();
                #[rustfmt::skip]
                args.handle.args_i([
                    "--overlay-src", &work_str,
                    "--overlay-src", &string,
                    "--ro-overlay", &format!("/{name}"),
                ])?;
            }
        }
    } else {
        debug!("Fabricating /etc");
        fab::etc::fabricate(&mut args.profile, &args.name);

        // Bin must run before lib so that bin can populate libraries
        debug!("Fabricating binaries");
        fab::bin::fabricate(&mut args.profile, &args.name, &args.handle)?;

        debug!("Fabricating /lib");
        fab::lib::fabricate(&mut args.profile, &args.name, &args.sys_dir, &args.handle)?;
    }

    debug!("Fabricating namespaces");
    fab::ns::fabricate(&mut args.profile, &args.handle)?;

    debug!("Fabricating /dev");
    fab::dev::fabricate(&mut args.profile, &args.handle)?;

    debug!("Writing fabricator cache: {}", cmd_cache.display());
    args.handle.cache_write(&cmd_cache)?;
    Ok(())
}
