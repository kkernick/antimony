use crate::{
    cli,
    shared::{Set, extend_option},
};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Networking IPC mediated via Landlock.
#[derive(Default, Deserialize, Serialize, PartialEq, Eq, Clone, Debug)]
#[serde(deny_unknown_fields, default)]
pub struct Ports {
    /// Ports the sandbox should be allowed bind to.
    pub bind: Option<Set<u16>>,

    /// Ports the sandbox should be allowed to connect to.
    pub connect: Option<Set<u16>>,
}

/// IPC mediated via xdg-dbus-proxy.
#[derive(Default, Deserialize, Serialize, PartialEq, Eq, Clone, Debug)]
#[serde(deny_unknown_fields, default)]
pub struct Ipc {
    /// Disable all IPC, regardless of what has been set.
    pub disable: Option<bool>,

    /// Provide the system bus. Defaults to false
    pub system_bus: Option<bool>,

    /// Provide the user bus directly. xdg-dbus-proxy is not run. Defaults to false.
    pub user_bus: Option<bool>,

    /// Apply SOF and SECCOMP rules to the proxy sandbox. By default, the proxy
    /// is exempt from such security measures as there is some implicit trust
    /// on the `xdg-dbus-proxy` and separation between its sandbox and the profile.
    ///
    /// However, you can lock down the proxy's sandbox should your threat model deem
    /// it necessary/beneficial.
    pub harden: Option<bool>,

    /// Freedesktop portals.
    pub portals: Option<Set<Portal>>,

    /// Busses that the sandbox can see, but not interact with.
    pub sees: Option<Set<String>>,

    /// Busses the sandbox can talk over.
    pub talks: Option<Set<String>>,

    /// Busses the sandbox owns.
    pub owns: Option<Set<String>>,

    /// Call semantics.
    pub calls: Option<Set<String>>,

    /// Sockets paths the sandbox should be allowed to connect to (Besides user/system/proxy)
    pub sockets: Option<Set<String>>,

    /// Landlock mediated ports.
    pub ports: Option<Ports>,
}
impl Ipc {
    /// Merge two IPC sets together.
    pub fn merge(&mut self, mut ipc: Self) {
        if self.disable.is_none() {
            self.disable = ipc.disable;
        }

        if self.system_bus.is_none() {
            self.system_bus = ipc.system_bus;
        }
        if self.user_bus.is_none() {
            self.user_bus = ipc.user_bus;
        }
        if self.harden.is_none() {
            self.harden = ipc.harden;
        }

        extend_option(&mut self.portals, ipc.portals);
        extend_option(&mut self.sees, ipc.sees);
        extend_option(&mut self.talks, ipc.talks);
        extend_option(&mut self.owns, ipc.owns);
        extend_option(&mut self.calls, ipc.calls);
        extend_option(&mut self.sockets, ipc.sockets);

        if let Some(ports) = ipc.ports.take() {
            if let Some(self_ports) = &mut self.ports {
                extend_option(&mut self_ports.bind, ports.bind);
                extend_option(&mut self_ports.connect, ports.connect);
            } else {
                self.ports = Some(ports);
            }
        }
    }

    /// Construct an IPC set from the command line.
    pub fn from_args(args: &mut cli::run::Args) -> Option<Self> {
        let mut ipc: Option<Self> = None;

        if let Some(portals) = args.portals.take() {
            ipc.get_or_insert_default().portals = Some(portals.into_iter().collect());
        }
        if let Some(see) = args.sees.take() {
            ipc.get_or_insert_default().sees = Some(see.into_iter().collect());
        }
        if let Some(talk) = args.talks.take() {
            ipc.get_or_insert_default().talks = Some(talk.into_iter().collect());
        }
        if let Some(own) = args.owns.take() {
            ipc.get_or_insert_default().owns = Some(own.into_iter().collect());
        }
        if let Some(call) = args.calls.take() {
            ipc.get_or_insert_default().calls = Some(call.into_iter().collect());
        }
        if let Some(sockets) = args.sockets.take() {
            ipc.get_or_insert_default().sockets = Some(sockets.into_iter().collect());
        }
        if let Some(bind) = args.bind.take() {
            ipc.get_or_insert_default()
                .ports
                .get_or_insert_default()
                .bind = Some(bind.into_iter().collect());
        }
        if let Some(connect) = args.connect.take() {
            ipc.get_or_insert_default()
                .ports
                .get_or_insert_default()
                .connect = Some(connect.into_iter().collect());
        }

        if args.user_bus {
            ipc.get_or_insert_default().user_bus = Some(true);
        }
        if args.system_bus {
            ipc.get_or_insert_default().system_bus = Some(true);
        }
        if args.disable_ipc {
            ipc.get_or_insert_default().disable = Some(true);
        }
        if args.harden_ipc {
            ipc.get_or_insert_default().harden = Some(true);
        }

        ipc
    }
}

/// A non-exhaustive list of Portals. Some may not be
/// implemented for certain Desktop Environments.
/// Not all applications use portals, even if they
/// are provided to the sandbox.
#[derive(Eq, Hash, PartialEq, Deserialize, Serialize, ValueEnum, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub enum Portal {
    Background = 0,
    Camera = 1,
    Clipboard = 2,
    Documents = 3,
    FileChooser = 4,
    GlobalShortcuts = 5,
    Inhibit = 6,
    Location = 7,
    Notifications = 8,
    OpenURI = 9,
    ProxyResolver = 10,
    Realtime = 11,
    ScreenCast = 12,
    Screenshot = 13,
    Settings = 14,
    Secret = 15,
    NetworkMonitor = 16,
    RemoteDesktop = 17,
    InputCapture = 18,
}
impl fmt::Display for Portal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Background => write!(f, "Background"),
            Self::Camera => write!(f, "Camera"),
            Self::Clipboard => write!(f, "Clipboard"),
            Self::Documents => write!(f, "Documents"),
            Self::FileChooser => write!(f, "FileChooser"),
            Self::GlobalShortcuts => write!(f, "GlobalShortcuts"),
            Self::Inhibit => write!(f, "Inhibit"),
            Self::Location => write!(f, "Location"),
            Self::Notifications => write!(f, "Notifications"),
            Self::OpenURI => write!(f, "OpenURI"),
            Self::ProxyResolver => write!(f, "ProxyResolver"),
            Self::Realtime => write!(f, "Realtime"),
            Self::ScreenCast => write!(f, "ScreenCast"),
            Self::Screenshot => write!(f, "Screenshot"),
            Self::Settings => write!(f, "Settings"),
            Self::Secret => write!(f, "Secret"),
            Self::NetworkMonitor => write!(f, "NetworkMonitor"),
            Self::RemoteDesktop => write!(f, "RemoteDesktop"),
            Self::InputCapture => write!(f, "InputCapture"),
        }
    }
}
