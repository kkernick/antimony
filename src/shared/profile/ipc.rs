use crate::{
    cli,
    shared::{ISet, format_iter},
};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};

/// IPC mediated via xdg-dbus-proxy.
#[derive(Default, Deserialize, Serialize, PartialEq, Eq, Clone)]
#[serde(deny_unknown_fields, default)]
pub struct Ipc {
    /// Disable all IPC, regardless of what has been set.
    pub disable: Option<bool>,

    /// Provide the system bus. Defaults to false
    pub system_bus: Option<bool>,

    /// Provide the user bus directly. xdg-dbus-proxy is not run. Defaults to false.
    pub user_bus: Option<bool>,

    /// Freedesktop portals.
    #[serde(skip_serializing_if = "ISet::is_empty")]
    pub portals: ISet<Portal>,

    /// Busses that the sandbox can see, but not interact with.
    #[serde(skip_serializing_if = "ISet::is_empty")]
    pub see: ISet<String>,

    /// Busses the sandbox can talk over.
    #[serde(skip_serializing_if = "ISet::is_empty")]
    pub talk: ISet<String>,

    /// Busses the sandbox owns.
    #[serde(skip_serializing_if = "ISet::is_empty")]
    pub own: ISet<String>,

    /// Call semantics.
    #[serde(skip_serializing_if = "ISet::is_empty")]
    pub call: ISet<String>,
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
        self.see.extend(ipc.see);
        self.talk.extend(ipc.talk);
        self.own.extend(ipc.own);
        self.call.extend(ipc.call);
    }

    /// Construct an IPC set from the command line.
    pub fn from_args(args: &mut cli::run::Args) -> Option<Self> {
        let mut ipc: Option<Self> = None;

        if let Some(portals) = args.portals.take() {
            ipc.get_or_insert_default().portals = portals.into_iter().collect();
        };
        if let Some(see) = args.see.take() {
            ipc.get_or_insert_default().see = see.into_iter().collect();
        };
        if let Some(talk) = args.talk.take() {
            ipc.get_or_insert_default().talk = talk.into_iter().collect();
        };
        if let Some(own) = args.own.take() {
            ipc.get_or_insert_default().own = own.into_iter().collect();
        };
        if let Some(call) = args.call.take() {
            ipc.get_or_insert_default().call = call.into_iter().collect();
        };

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

    /// Get info about the IPC set.
    pub fn info(&self) {
        println!("\t- IPC mediated via xdg-dbus-proxy");
        if !self.portals.is_empty() {
            println!("\t\t- Portals: {}", format_iter(self.portals.iter()));
        }
        if !self.talk.is_empty() {
            println!("\t\t- Talk: {}", format_iter(self.talk.iter()));
        }
        if !self.see.is_empty() {
            println!("\t\t- Visible: {}", format_iter(self.see.iter()));
        }
        if !self.own.is_empty() {
            println!("\t\t- Owns: {}", format_iter(self.own.iter()));
        }
        if !self.call.is_empty() {
            println!("\t\t- Calls via: {}", format_iter(self.call.iter()));
        }
    }
}

/// A non-exhaustive list of Portals. Some may not be
/// implemented for certain Desktop Environments.
/// Not all applications use portals, even if they
/// are provided to the sandbox.
#[derive(Debug, Eq, Hash, PartialEq, Deserialize, Serialize, ValueEnum, Clone)]
#[serde(deny_unknown_fields)]
pub enum Portal {
    Background,
    Camera,
    Clipboard,
    Documents,
    FileChooser,
    GlobalShortcuts,
    Inhibit,
    Location,
    Notifications,
    OpenURI,
    ProxyResolver,
    Realtime,
    ScreenCast,
    Screenshot,
    Settings,
    Secret,
    NetworkMonitor,
}
impl std::fmt::Display for Portal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Portal::Background => write!(f, "Background"),
            Portal::Camera => write!(f, "Camera"),
            Portal::Clipboard => write!(f, "Clipboard"),
            Portal::Documents => write!(f, "Documents"),
            Portal::FileChooser => write!(f, "FileChooser"),
            Portal::GlobalShortcuts => write!(f, "GlobalShortcuts"),
            Portal::Inhibit => write!(f, "Inhibit"),
            Portal::Location => write!(f, "Location"),
            Portal::Notifications => write!(f, "Notifications"),
            Portal::OpenURI => write!(f, "OpenURI"),
            Portal::ProxyResolver => write!(f, "ProxyResolver"),
            Portal::Realtime => write!(f, "Realtime"),
            Portal::ScreenCast => write!(f, "ScreenCast"),
            Portal::Screenshot => write!(f, "Screenshot"),
            Portal::Settings => write!(f, "Settings"),
            Portal::Secret => write!(f, "Secret"),
            Portal::NetworkMonitor => write!(f, "NetworkMonitor"),
        }
    }
}
