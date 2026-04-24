//! Antimony uses xdg-dbus-proxy to proxy the user bus. It does this by spawning an associated process
//! that hooks onto the sandbox's user bus socket, and mediating the calls that come in.

use crate::{
    fab::{
        get_libraries,
        lib::{add_sof, mount_roots},
    },
    setup::syscalls,
    shared::{
        Set,
        env::{CACHE_DIR, RUNTIME_DIR, RUNTIME_STR},
        profile::{Profile, ipc::Portal, ns::Namespace},
    },
    timer,
};
use anyhow::Result;
use inotify::WatchMask;
use log::debug;
use rayon::prelude::*;
use spawn::{Spawner, StreamMode};
use std::{
    borrow::Cow,
    env,
    fs::{self, File},
    io::Write,
    os::fd::AsRawFd,
    path::Path,
};
use temp::Temp;
use user::as_real;

/// Get the Spawner used to run Proxy.
pub fn run(
    sys_dir: &Path,
    profile: &Profile,
    instance: &Temp,
    info: &Path,
    id: &str,
) -> Result<Spawner> {
    let runtime = RUNTIME_DIR.to_string_lossy();
    let cache = CACHE_DIR.join(".proxy");
    let sof = cache.join("sof");
    let app_dir = RUNTIME_DIR.join("app").join(id);
    let proxy = instance.full().join("proxy");

    timer!("::directory_setup", {
        as_real!(Result<()>, {
            if !proxy.exists() {
                fs::create_dir_all(&proxy)?;
            }
            if !app_dir.exists() {
                fs::create_dir_all(&app_dir)?;
            }
            Ok(())
        })??;
    });

    // Create an SOF for the proxy.
    // It's shared between every application and instance.
    // The attack surface for this is bordering on paranoid, as
    // we don't trust the proxy, or expect the sandbox to somehow
    // access the proxy's sandbox through the user bus. Still, we
    // have the means to do this, so might as well. The real
    // paranoia is below.
    if !sof.exists() {
        fs::create_dir_all(&sof)?;
        timer!("::sof", {
            let libraries = get_libraries("/usr/bin/xdg-dbus-proxy")?;
            libraries
                .into_par_iter()
                .try_for_each(|library| add_sof(&sof, Cow::Owned(library), &cache))?;
        });
    }

    let proxy = timer!("::spawner", {
        #[rustfmt::skip]
        let proxy = Spawner::abs("/usr/bin/bwrap")
        .name("proxy")
        .error(StreamMode::Log(log::Level::Error))
        .mode(user::Mode::Real).args([
            "--new-session",
            "--ro-bind", "/usr/bin/xdg-dbus-proxy", "/usr/bin/xdg-dbus-proxy",
            "--clearenv",
            "--disable-userns",
            "--assert-userns-disabled",
            "--unshare-all",
            "--unshare-user",
            "--die-with-parent",
            "--dir", &runtime,
            "--bind", &format!("{runtime}/bus"), &format!("{runtime}/bus"),
            "--ro-bind", &info.to_string_lossy(), "/.flatpak-info",
            "--symlink", "/.flatpak-info", &format!("{runtime}/flatpak-info"),
            "--bind", &proxy.to_string_lossy(), &format!("{runtime}/app/{id}"),
        ]);

        let sof_str = sof.to_string_lossy();
        mount_roots(&sof_str, &proxy)?;
        proxy
    });

    timer!("::post", {
        proxy.args_i([
            "--",
            "/usr/bin/xdg-dbus-proxy",
            &env::var("DBUS_SESSION_BUS_ADDRESS")?,
            &app_dir.join("bus").to_string_lossy(),
            "--filter",
        ]);

        if log::log_enabled!(log::Level::Debug) {
            proxy.arg_i("--log");
            proxy.output_i(StreamMode::Log(log::Level::Debug));
        }
    });

    // We cache the proxy's arguments directly.
    let cache = sys_dir.join("proxy.cache");
    if cache.exists() {
        proxy.cache_read(&cache)?;
    } else {
        timer!("::args", {
            proxy.cache_start()?;

            let permit_call = |portal: &str| -> String {
                let path = portal.replace(".", "/").to_ascii_lowercase();
                format!("--call={portal}=org.freedesktop.DBus.Properties.*@{path}")
            };

            if let Some(ipc) = &profile.ipc {
                if !ipc.portals.is_empty() {
                    let desktop = "org.freedesktop.portal.Desktop";
                    let path = "/org/freedesktop/portal/desktop";
                    proxy.args_i([
                        format!("--call={desktop}=org.freedesktop.DBus.Properties.*@{path}/*"),
                        format!(
                            "--call={desktop}=org.freedesktop.DBus.Introspectable.Introspect@{path}"
                        ),
                    ]);

                    for portal in &ipc.portals {
                        if portal == &Portal::Settings {
                            proxy.arg_i(format!("--broadcast={desktop}=org.freedesktop.portal.Settings.SettingChanged@{path}"));
                        }
                        proxy.args_i([
                            format!("--call={desktop}=org.freedesktop.portal.{portal}.*@{path}"),
                            format!("--talk=org.freedesktop.portal.{portal}"),
                        ]);
                    }
                }
                for portal in &ipc.sees {
                    proxy.args_i([format!("--see={portal}"), permit_call(portal)]);
                }
                for portal in &ipc.talks {
                    proxy.args_i([format!("--talk={portal}"), permit_call(portal)]);
                }
                for portal in &ipc.owns {
                    proxy.args_i([format!("--own={portal}"), permit_call(portal)]);
                }
                for portal in &ipc.calls {
                    proxy.arg_i(format!("--call={portal}"));
                }
            }
            proxy.cache_write(&cache)?;
        })
    }
    Ok(proxy)
}

pub fn setup(args: &mut super::Args) -> Result<()> {
    // Scope the lock
    let ipc = {
        if let Some(ipc) = &args.profile.ipc {
            ipc.clone()
        } else {
            return Ok(());
        }
    };

    if ipc.disable.unwrap_or(false) {
        return Ok(());
    }

    debug!("Setting up proxy");
    let runtime = RUNTIME_STR.as_str();

    // Add the system bus.
    if ipc.system_bus.unwrap_or(false) {
        args.handle.args_i([
            "--ro-bind",
            "/var/run/dbus/system_bus_socket",
            "/var/run/dbus/system_bus_socket",
        ]);
    }

    let id = &args.id;
    let instance_dir = args.instance.full();
    let instance_dir_str = instance_dir.to_string_lossy();
    let info = instance_dir.join(".flatpak-info");

    debug!("Setting up user bus");
    // Either mount the bus directly
    if ipc.user_bus.unwrap_or(false) {
        args.handle.args_i([
            "--ro-bind",
            &format!("{}/bus", RUNTIME_STR.as_str()),
            &format!("{}/bus", RUNTIME_STR.as_str()),
        ]);

    // Or mediate via the proxy.
    } else {
        let proxy = timer!(
            "::run",
            run(&args.sys_dir, &args.profile, args.instance, &info, id)
        )?;

        if !args.args.dry {
            // This is *very* paranoid, but the proxy gets confined by its
            // own policy when SECCOMP is enabled.
            if let Some(policy) = args.profile.seccomp {
                timer!("::seccomp", {
                    syscalls::install_filter(
                        "xdg-dbus-proxy",
                        args.instance,
                        policy,
                        &Set::default(),
                        &proxy,
                        &args.handle,
                        false,
                    )?
                })
            }

            // Create the flatpak-info, but don't bother if we're running dry.
            timer!("::flatpak_info", {
                let namespaces = args.profile.namespaces.clone();

                // https://docs.flatpak.org/en/latest/flatpak-command-reference.html
                #[rustfmt::skip]
                let mut info_contents: Vec<String> = vec![
                    "[Application]",
                    &format!("name={id}"),
                    "[Instance]",
                    &format!("instance-id={}", args.instance.name()),
                    "app-path=/usr",
                    "[Context]",
                    "sockets=session-bus;system-bus;",
                ].into_iter().map(|e| e.to_string()).collect();

                if namespaces.contains(&Namespace::Net) || namespaces.contains(&Namespace::All) {
                    info_contents.push("shared=network;".to_string());
                }

                as_real!(Result<()>, {
                    debug!("Creating flatpak info");
                    if let Some(parent) = info.parent()
                        && !parent.exists()
                    {
                        fs::create_dir_all(parent)?;
                    }
                    let out = fs::File::create_new(&info)?;
                    write!(&out, "{}", info_contents.join("\n"))?;
                    Ok(())
                })??;

                #[rustfmt::skip]
                args.handle.args_i([
                    "--bind", &format!("{runtime}/doc"), &format!("{runtime}/doc"),
                    "--ro-bind", "/run/dbus", "/run/dbus",
                    "--setenv", "DBUS_SESSION_BUS_ADDRESS", &format!("unix:path=/run/user/{}/bus", user::USER.real),
                    "--ro-bind", &format!("{instance_dir_str}/.flatpak-info"), "/.flatpak-info",
                    "--symlink", "/.flatpak-info", &format!("{runtime}/flatpak-info"),
                ]);
            });

            timer!("::flapak_dir", {
                debug!("Creating flatpak directory");
                let flatpak_dir = RUNTIME_DIR.join(".flatpak").join(args.instance.name());

                let file = as_real!(Result<File>, {
                    if !flatpak_dir.exists() {
                        fs::create_dir_all(&flatpak_dir)?;
                    }
                    let file = File::create(flatpak_dir.join("bwrapinfo.json"))?;
                    Ok(file)
                })??;

                args.handle
                    .args_i(["--json-status-fd", &format!("{}", file.as_raw_fd())]);
                args.handle.fd_i(file);
            });

            // Watch for the proxy to expose its bus to give to the sandbox.
            debug!("Creating proxy watch");
            as_real!(Result<()>, {
                args.watches.insert(
                    args.inotify
                        .watches()
                        .add(instance_dir.join("proxy"), WatchMask::CREATE)?,
                );
                Ok(())
            })??;

            args.handle.associate(proxy.spawn()?);
        }

        args.handle.args_i([
            "--ro-bind",
            &format!("{instance_dir_str}/proxy/bus"),
            &format!("{runtime}/bus"),
        ]);
    }

    Ok(())
}
