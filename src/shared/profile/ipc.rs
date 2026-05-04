use std::fmt;

use crate::{cli, shared::Set};
use bilrost::{Enumeration, Message};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};

/// IPC mediated via xdg-dbus-proxy.
#[derive(Default, Deserialize, Serialize, PartialEq, Eq, Clone, Debug, Message)]
#[serde(deny_unknown_fields, default)]
pub struct Ipc {
    /// Disable all IPC, regardless of what has been set.
    pub disable: Option<bool>,

    /// Provide the system bus. Defaults to false
    pub system_bus: Option<bool>,

    /// Provide the user bus directly. xdg-dbus-proxy is not run. Defaults to false.
    pub user_bus: Option<bool>,

    /// Freedesktop portals.
    #[serde(skip_serializing_if = "Set::is_empty")]
    pub portals: Set<Portal>,

    /// Busses that the sandbox can see, but not interact with.
    #[serde(skip_serializing_if = "Set::is_empty")]
    pub sees: Set<String>,

    /// Busses the sandbox can talk over.
    #[serde(skip_serializing_if = "Set::is_empty")]
    pub talks: Set<String>,

    /// Busses the sandbox owns.
    #[serde(skip_serializing_if = "Set::is_empty")]
    pub owns: Set<String>,

    /// Call semantics.
    #[serde(skip_serializing_if = "Set::is_empty")]
    pub calls: Set<String>,
}
impl Ipc {
    /// Merge two IPC sets together.
    pub fn merge(&mut self, ipc: Self) {
        if self.disable.is_none() {
            self.disable = ipc.disable;
        }

        if self.system_bus.is_none() {
            self.system_bus = ipc.system_bus;
        }
        if self.user_bus.is_none() {
            self.user_bus = ipc.user_bus;
        }

        self.portals.extend(ipc.portals);
        self.sees.extend(ipc.sees);
        self.talks.extend(ipc.talks);
        self.owns.extend(ipc.owns);
        self.calls.extend(ipc.calls);
    }

    /// Construct an IPC set from the command line.
    pub fn from_args(args: &mut cli::run::Args) -> Option<Self> {
        let mut ipc: Option<Self> = None;

        if let Some(portals) = args.portals.take() {
            ipc.get_or_insert_default().portals = portals.into_iter().collect();
        }
        if let Some(see) = args.sees.take() {
            ipc.get_or_insert_default().sees = see.into_iter().collect();
        }
        if let Some(talk) = args.talks.take() {
            ipc.get_or_insert_default().talks = talk.into_iter().collect();
        }
        if let Some(own) = args.owns.take() {
            ipc.get_or_insert_default().owns = own.into_iter().collect();
        }
        if let Some(call) = args.calls.take() {
            ipc.get_or_insert_default().calls = call.into_iter().collect();
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

        ipc
    }
}

/// A non-exhaustive list of Portals. Some may not be
/// implemented for certain Desktop Environments.
/// Not all applications use portals, even if they
/// are provided to the sandbox.
#[derive(Debug, Eq, Hash, PartialEq, Deserialize, Serialize, ValueEnum, Clone, Enumeration)]
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
        }
    }
}
