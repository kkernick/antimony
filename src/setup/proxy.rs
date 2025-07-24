use crate::{
    aux::{
        env::{AT_HOME, RUNTIME_DIR, RUNTIME_STR},
        path::user_dir,
        profile::{Namespace, Portal, Profile},
        syscalls,
    },
    fab::lib::get_libraries,
};
use anyhow::Result;
use inotify::WatchMask;
use log::debug;
use rayon::prelude::*;
use spawn::Spawner;
use std::{
    borrow::Cow,
    fs::{self, File},
    io::Write,
    path::Path,
};
use user;
use which::which;

pub fn run(
    sys_dir: &Path,
    profile: &mut Profile,
    instance: &str,
    info: &Path,
    id: &str,
) -> Result<Spawner> {
    let saved = user::save()?;

    let runtime = RUNTIME_DIR.to_string_lossy();
    let resolve = which("xdg-dbus-proxy")?.to_string_lossy().into_owned();
    let instance_dir = String::from(".proxy");

    user::set(user::Mode::Effective)?;
    // Create an SOF for the proxy.
    // It's shared between every application and instance.
    // Performed before we drop to the user.
    let sof = Path::new(AT_HOME.as_path())
        .join("cache")
        .join(instance_dir);
    if !sof.exists() {
        let libraries = get_libraries(Cow::Borrowed(&resolve))?;

        libraries
            .into_par_iter()
            .map(|library| -> Result<()> {
                let sof_path = match library.rfind('/') {
                    Some(i) => sof.join(&library[i + 1..]),
                    None => sof.join(&library),
                };

                if let Some(parent) = sof_path.parent() {
                    fs::create_dir_all(parent)?;
                    let canon = fs::canonicalize(&library)?;
                    if fs::hard_link(&canon, &sof_path).is_err() {
                        fs::copy(canon, sof_path)?;
                    }
                }
                Ok(())
            })
            // Collect in case of any errors.
            .collect::<Result<Vec<_>>>()?;
    }
    user::set(user::Mode::Real)?;

    let sof_str = sof.to_string_lossy();

    debug!("Creating proxy directory");
    let proxy = user_dir(instance).join("proxy");
    fs::create_dir_all(&proxy)?;

    #[rustfmt::skip]
    let mut proxy = Spawner::new("bwrap")
        .mode(user::Mode::Real).args([
            "--new-session",
            "--ro-bind", &resolve, &resolve,
            "--clearenv",
            "--disable-userns",
            "--assert-userns-disabled",
            "--unshare-all",
            "--unshare-user",
            "--die-with-parent",
            "--bind", &runtime, &runtime,
            "--ro-bind", &info.to_string_lossy(), "/.flatpak-info",
            "--symlink", "/.flatpak-info", &format!("{runtime}/flatpak-info"),
            "--bind", &proxy.to_string_lossy(), &format!("{runtime}/app/{id}"),
        ])?;

    let app_dir = RUNTIME_DIR.join("app").join(id);
    fs::create_dir_all(&app_dir)?;

    #[rustfmt::skip]
    proxy.args_i([
        "--overlay-src", &format!("{sof_str}"), "--tmp-overlay", "/usr/lib",
        "--symlink", "/usr/lib", "/usr/lib64",
        "--symlink", "/usr/lib", "/lib",
        "--symlink", "/usr/lib", "/lib64",
    ])?;

    // Setup SECCOMP.
    if let Some(policy) = profile.seccomp {
        proxy.seccomp_i(syscalls::new("xdg-dbus-proxy", instance, policy, &None)?);
    }

    proxy.args_i([
        "--",
        &resolve,
        &std::env::var("DBUS_SESSION_BUS_ADDRESS")?,
        &app_dir.join("bus").to_string_lossy(),
        "--filter",
    ])?;

    if let Ok(log) = std::env::var("RUST_LOG") {
        if log == "debug" {
            proxy.arg_i("--log")?;
        }
    }

    let cache = sys_dir.join("proxy.cache");
    if cache.exists() {
        user::set(user::Mode::Effective)?;
        proxy.cache_read(&cache)?;
    } else {
        proxy.cache_start()?;

        let permit_call = |portal: &str| -> String {
            let path = portal.replace(".", "/").to_ascii_lowercase();
            format!("--call={portal}=org.freedesktop.DBus.Properties.*@{path}")
        };

        if let Some(ipc) = profile.ipc.take() {
            if !ipc.portals.is_empty() {
                let desktop = "org.freedesktop.portal.Desktop";
                let path = "/org/freedesktop/portal/desktop";
                proxy.args_i([
                    format!("--call={desktop}=org.freedesktop.DBus.Properties.*@{path}/*"),
                    format!(
                        "--call={desktop}=org.freedesktop.DBus.Introspectable.Introspect@{path}"
                    ),
                ])?;

                for portal in &ipc.portals {
                    if portal == &Portal::Settings {
                        proxy.arg_i(format!("--broadcast={desktop}=org.freedesktop.portal.Settings.SettingChanged@{path}"))?;
                    }
                    proxy.args_i([
                        format!("--call={desktop}=org.freedesktop.portal.{portal:?}.*@{path}"),
                        format!("--talk=org.freedesktop.portal.{portal:?}"),
                    ])?;
                }
            }
            for portal in &ipc.see {
                proxy.args_i([format!("--see={portal}"), permit_call(portal)])?;
            }
            for portal in &ipc.talk {
                proxy.args_i([format!("--talk={portal}"), permit_call(portal)])?;
            }
            for portal in &ipc.own {
                proxy.args_i([format!("--own={portal}"), permit_call(portal)])?;
            }
            for portal in &ipc.call {
                proxy.arg_i(format!("--call={portal}"))?;
            }
        }

        user::set(user::Mode::Effective)?;
        proxy.cache_write(&cache)?;
    }

    user::restore(saved)?;
    Ok(proxy)
}

pub fn setup(args: &mut super::Args) -> Result<()> {
    debug!("Setting up proxy");
    let runtime = RUNTIME_STR.as_str();

    // Run the proxy
    if let Some(ipc) = &args.profile.ipc {
        // Add the system bus.
        let system_bus = ipc.system_bus.unwrap_or(false);
        if system_bus {
            args.handle.args_i([
                "--ro-bind",
                "/var/run/dbus/system_bus_socket",
                "/var/run/dbus/system_bus_socket",
            ])?;
        }

        let instance = &args.instance;
        let id = args.profile.id(&args.name);

        // Create the flatpak-info
        user::set(user::Mode::Real)?;
        debug!("Creating flatpak info");
        let info = user_dir(instance).join(".flatpak-info");
        let out = fs::File::create_new(&info)?;

        // https://docs.flatpak.org/en/latest/flatpak-command-reference.html
        #[rustfmt::skip]
        let mut info_contents: Vec<String> = vec![
            "[Application]",
            &format!("name={id}"),
            "[Instance]",
            &format!("instance-id={instance}"),
            "app-path=/usr",
            "[Context]",
            "sockets=session-bus;system-bus;",
        ].into_iter().map(|e| e.to_string()).collect();
        if let Some(ns) = &args.profile.namespaces {
            if ns.contains(&Namespace::Net) {
                info_contents.push("shared=network;".to_string());
            }
        }
        write!(&out, "{}", info_contents.join("\n"))?;

        // Add the required files.
        let user_dir_str = user_dir(&args.instance).to_string_lossy().into_owned();
        #[rustfmt::skip]
        args.handle.args_i([
            "--bind", &format!("{runtime}/doc"), &format!("{runtime}/doc"),
            "--ro-bind", "/run/dbus", "/run/dbus",
            "--setenv", "DBUS_SESSION_BUS_ADDRESS", &format!("unix:path=/run/user/{}/bus", user::USER.real),
            "--ro-bind", &format!("{user_dir_str}/.flatpak-info"), "/.flatpak-info",
            "--symlink", "/.flatpak-info", &format!("{runtime}/flatpak-info"),
        ])?;

        // Add the bwrapinfo status.
        debug!("Creating flatpak directory");
        let flatpak_dir = RUNTIME_DIR.join(".flatpak").join(instance);
        fs::create_dir_all(&flatpak_dir)?;
        args.handle.fd_arg_i(
            "--json-status-fd",
            File::create(flatpak_dir.join("bwrapinfo.json"))?,
        )?;
        user::revert()?;

        let user_bus = ipc.user_bus.unwrap_or(false);
        // Either mount the bus directly
        if user_bus {
            args.handle.args_i([
                "--ro-bind",
                &format!("{}/bus", RUNTIME_STR.as_str()),
                &format!("{}/bus", RUNTIME_STR.as_str()),
            ])?;

        // Or mediate via the proxy.
        } else if !ipc.disable.unwrap_or(false) {
            let proxy = run(&args.sys_dir, &mut args.profile, &args.instance, &info, &id)?;
            args.handle.args_i([
                "--ro-bind",
                &format!("{user_dir_str}/proxy/bus"),
                &format!("{runtime}/bus"),
            ])?;

            if !args.args.dry {
                user::set(user::Mode::Real)?;
                if !args.args.dry {
                    debug!("Creating proxy watch");
                    args.watches.insert(
                        args.inotify
                            .watches()
                            .add(user_dir(&args.instance).join("proxy"), WatchMask::CREATE)?,
                    );
                }
                user::revert()?;
                args.handle.associate(proxy.spawn()?);
            }
        }
    }

    user::revert()?;
    Ok(())
}
