pub mod bin;
pub mod dev;
pub mod etc;
pub mod features;
pub mod files;
pub mod lib;
pub mod ns;

use crate::shared::env::HOME;
use log::debug;
use std::{borrow::Cow, env, path::Path};

pub fn resolve_env(string: Cow<'_, str>) -> Cow<'_, str> {
    if string.contains('$') {
        let mut resolved = String::new();
        let mut chars = string.chars().peekable();

        while let Some(ch) = chars.next() {
            if ch == '$' {
                let mut var_name = String::new();
                while let Some(&next) = chars.peek() {
                    if next.is_ascii_uppercase() || next.is_ascii_digit() || next == '_' {
                        var_name.push(next);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if !var_name.is_empty() {
                    let val = match var_name.as_str() {
                        "UID" => format!("{}", user::USER.real),
                        name => env::var(name).unwrap_or_else(|_| format!("${name}")),
                    };
                    resolved.push_str(&val);
                } else {
                    resolved.push('$');
                }
            } else {
                resolved.push(ch)
            }
        }
        Cow::Owned(resolved)
    } else {
        string
    }
}

/// Resolve environment variables in paths.
#[inline]
pub fn resolve(mut path: Cow<'_, str>) -> Cow<'_, str> {
    if path.starts_with('~') {
        path = Cow::Owned(path.replace("~", "/home/antimony"));
    }
    resolve_env(path)
}

/// Localize a home path to /home/antimony
#[inline]
pub fn localize_home<'a>(path: &'a str) -> Cow<'a, str> {
    if path.starts_with("/home") {
        Cow::Owned(path.replace(HOME.as_str(), "/home/antimony"))
    } else {
        Cow::Borrowed(path)
    }
}

/// Ensure ~ points to /home/antimony
pub fn localize_path(file: &str, home: bool) -> Result<(Option<Cow<'_, str>>, String), nix::Error> {
    let (source, dest) = if let Some((source, dest)) = file.split_once('=') {
        (resolve(Cow::Borrowed(source)), Cow::Borrowed(dest))
    } else {
        let mut resolved = resolve(Cow::Borrowed(file));
        if home && !resolved.starts_with("/home") {
            resolved = Cow::Owned(format!("{}/{resolved}", HOME.as_str()));
        }
        (resolved.clone(), resolved)
    };
    let dest = localize_home(&dest);

    Ok(
        if user::run_as!(user::Mode::Real, { Path::new(source.as_ref()).exists() }) {
            debug!("{source} => {dest}");
            (Some(source), dest.into_owned())
        } else {
            debug!("{source} (does not exist) => {dest}");
            (None, dest.into_owned())
        },
    )
}
