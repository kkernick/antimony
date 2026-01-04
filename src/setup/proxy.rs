use crate::{
    fab::{get_libraries, lib::add_sof},
    setup::syscalls,
    shared::{
        ISet,
        env::{CACHE_DIR, RUNTIME_DIR, RUNTIME_STR},
        path::user_dir,
        profile::{Namespace, Portal, Profile},
    },
    timer,
};
use anyhow::Result;
use inotify::WatchMask;
use log::debug;
use parking_lot::Mutex;
use rayon::prelude::*;
use spawn::{Spawner, StreamMode};
use std::{
    borrow::Cow,
    env,
    fs::{self, File},
    io::Write,
    os::fd::AsRawFd,
    path::Path,
    sync::Arc,
};
use user::{as_effective, as_real};

pub fn run(
    sys_dir: &Path,
    profile: &Mutex<Profile>,
    instance: &str,
    info: &Path,
    id: &str,
    dry: bool,
) -> Result<Spawner> {
    let runtime = RUNTIME_DIR.to_string_lossy();
    let cache = CACHE_DIR.join(".proxy");
    let sof = cache.join("sof");
    let app_dir = RUNTIME_DIR.join("app").join(id);
    let proxy = user_dir(instance).join("proxy");

    timer!("::directory_setup", {
        if !proxy.exists() {
            as_real!(fs::create_dir_all(&proxy))??;
        }
        if !app_dir.exists() {
            as_real!(fs::create_dir_all(&app_dir))??;
        }
    });

    // Create an SOF for the proxy.
    // It's shared between every application and instance.
    // Performed before we drop to the user.
    if !sof.exists() {
        as_effective!(fs::create_dir_all(&sof))??;

        timer!("::sof", {
            let libraries = get_libraries(Cow::Borrowed("/usr/bin/xdg-dbus-proxy"), Some(&cache))?;
            libraries
                .into_par_iter()
                .try_for_each(|library| add_sof(&sof, Cow::Owned(library), &cache, "/usr"))?;
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
        ])?;

        let sof_str = sof.to_string_lossy();
        proxy.args_i(["--ro-bind-try", &format!("{sof_str}/lib"), "/usr/lib"])?;
        let path = &format!("{sof_str}/lib64");
        if Path::new(path).exists() {
            proxy.args_i(["--ro-bind-try", path, "/usr/lib64"])?;
        } else {
            proxy.args_i(["--symlink", "/usr/lib", "/usr/lib64"])?;
        }

        #[rustfmt::skip]
        proxy.args_i([
            "--symlink", "/usr/lib", "/lib",
            "--symlink", "/usr/lib64","/lib64",
        ])?;
        proxy
    });

    // Setup SECCOMP.
    if !dry && let Some(policy) = profile.lock().seccomp {
        timer!("::seccomp", {
            syscalls::install_filter("xdg-dbus-proxy", instance, policy, &ISet::default(), &proxy)?
        })
    }

    timer!("::post", {
        proxy.args_i([
            "--",
            "/usr/bin/xdg-dbus-proxy",
            &env::var("DBUS_SESSION_BUS_ADDRESS")?,
            &app_dir.join("bus").to_string_lossy(),
            "--filter",
        ])?;

        if log::log_enabled!(log::Level::Debug) {
            proxy.arg_i("--log")?;
            proxy.output_i(StreamMode::Log(log::Level::Debug));
        }
    });

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

            if let Some(ipc) = &profile.lock().ipc {
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
                            format!("--call={desktop}=org.freedesktop.portal.{portal}.*@{path}"),
                            format!("--talk=org.freedesktop.portal.{portal}"),
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
            as_effective!(proxy.cache_write(&cache))??;
        })
    }
    Ok(proxy)
}

pub fn setup(args: Arc<super::Args>) -> Result<Option<Vec<Cow<'static, str>>>> {
    // Run the proxy
    let ipc = {
        let lock = args.profile.lock();
        if let Some(ipc) = &lock.ipc {
            ipc.clone()
        } else {
            return Ok(None);
        }
    };

    if ipc.disable.unwrap_or(false) {
        return Ok(None);
    }

    let mut arguments = Vec::new();

    debug!("Setting up proxy");
    let runtime = RUNTIME_STR.as_str();

    // Add the system bus.
    let system_bus = ipc.system_bus.unwrap_or(false);
    if system_bus {
        arguments.extend(
            [
                "--ro-bind",
                "/var/run/dbus/system_bus_socket",
                "/var/run/dbus/system_bus_socket",
            ]
            .map(Cow::Borrowed),
        );
    }

    let instance = args.instance.name();
    let id = &args.id;
    let instance_dir = args.instance.full();
    let instance_dir_str = instance_dir.to_string_lossy();
    let info = user_dir(instance).join(".flatpak-info");

    // Create the flatpak-info
    if !args.args.dry {
        timer!("::flatpak_info", {
            let namespaces = {
                let lock = args.profile.lock();
                lock.namespaces.clone()
            };

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
            arguments.extend([
                "--bind", &format!("{runtime}/doc"), &format!("{runtime}/doc"),
                "--ro-bind", "/run/dbus", "/run/dbus",
                "--setenv", "DBUS_SESSION_BUS_ADDRESS", &format!("unix:path=/run/user/{}/bus", user::USER.real),
                "--ro-bind", &format!("{instance_dir_str}/.flatpak-info"), "/.flatpak-info",
                "--symlink", "/.flatpak-info", &format!("{runtime}/flatpak-info"),
            ].map(String::from).map(Cow::Owned));
        });

        timer!("::flapak_dir", {
            debug!("Creating flatpak directory");
            let flatpak_dir = RUNTIME_DIR.join(".flatpak").join(instance);

            let file = as_real!(Result<File>, {
                if !flatpak_dir.exists() {
                    fs::create_dir_all(&flatpak_dir)?;
                }
                let file = File::create(flatpak_dir.join("bwrapinfo.json"))?;
                Ok(file)
            })??;

            arguments.extend(
                ["--json-status-fd", &format!("{}", file.as_raw_fd())]
                    .map(String::from)
                    .map(Cow::Owned),
            );
            args.handle.fd_i(file);
        });
    }

    debug!("Setting up user bus");
    let user_bus = ipc.user_bus.unwrap_or(false);
    // Either mount the bus directly
    if user_bus {
        arguments.extend(
            [
                "--ro-bind",
                &format!("{}/bus", RUNTIME_STR.as_str()),
                &format!("{}/bus", RUNTIME_STR.as_str()),
            ]
            .map(String::from)
            .map(Cow::Owned),
        );

    // Or mediate via the proxy.
    } else {
        let proxy = timer!(
            "::run",
            run(
                &args.sys_dir,
                &args.profile,
                args.instance.name(),
                &info,
                id,
                args.args.dry,
            )
        )?;
        arguments.extend(
            [
                "--ro-bind",
                &format!("{instance_dir_str}/proxy/bus"),
                &format!("{runtime}/bus"),
            ]
            .map(String::from)
            .map(Cow::Owned),
        );

        if !args.args.dry {
            debug!("Creating proxy watch");
            as_real!(Result<()>, {
                args.watches.insert(args.inotify.lock().watches().add(
                    user_dir(args.instance.name()).join("proxy"),
                    WatchMask::CREATE,
                )?);
                Ok(())
            })??;

            args.handle.associate(proxy.spawn()?);
            return Ok(Some(arguments));
        }
    }

    Ok(None)
}
