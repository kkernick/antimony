//! A simple implementation of xdg-open, without requiring Bash.

use ahash::{HashMap, HashMapExt};
use anyhow::{Result, anyhow};
use dbus::{
    Message,
    arg::Variant,
    blocking::{BlockingSender, LocalConnection},
    strings::{BusName, Interface, Member},
};
use std::{env, fs::File, os::fd::OwnedFd, path::Path, time::Duration};

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let arg = match args.get(1) {
        Some(arg) => arg,
        None => return Err(anyhow!("Invalid command line!")),
    };

    let (uri, member) = if Path::new(arg).exists() {
        (false, Member::from("OpenFile\0"))
    } else {
        (true, Member::from("OpenURI\0"))
    };

    let connection = LocalConnection::new_session()?;
    let msg = Message::new_method_call(
        BusName::from("org.freedesktop.portal.Desktop\0"),
        dbus::Path::from("/org/freedesktop/portal/desktop\0"),
        Interface::from("org.freedesktop.portal.OpenURI\0"),
        member,
    );

    if let Ok(msg) = msg {
        let args: HashMap<&str, Variant<Box<dyn dbus::arg::RefArg>>> = HashMap::new();
        if uri {
            connection
                .send_with_reply_and_block(msg.append3("", arg, args), Duration::from_mins(1))?;
        } else {
            let fd = OwnedFd::from(File::open(arg)?);
            connection
                .send_with_reply_and_block(msg.append3("", fd, args), Duration::from_mins(1))?;
        }
    } else {
        return Err(anyhow!("Failed to send message to OpenURI Portal!"));
    }

    Ok(())
}
