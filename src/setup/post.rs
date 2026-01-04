use crate::shared::{
    env::{HOME, PWD},
    profile::FileMode,
};
use anyhow::Result;
use log::debug;
use std::{fs, path::Path};
use user::as_real;

pub fn setup(args: &mut super::Args) -> Result<Vec<String>> {
    debug!("Setting up post arguments");
    let mut post_args = Vec::new();

    post_args.append(&mut args.profile.lock().arguments);
    if let Some(passthrough) = &args.args.passthrough {
        post_args.extend(passthrough.iter().cloned());
    }

    if !post_args.is_empty() {
        let operation = match args.profile.lock().files.take() {
            Some(mut files) => match files.passthrough.take() {
                Some(passthrough) => passthrough,
                None => FileMode::ReadOnly,
            },
            None => FileMode::ReadOnly,
        };

        for arg in &mut post_args {
            let abs_arg = if as_real!(PWD.join(&arg).exists())? {
                PWD.join(&arg).to_string_lossy().into_owned()
            } else {
                arg.to_string()
            };

            if as_real!(Path::new(&abs_arg).exists())? || abs_arg.starts_with("file://") {
                let file = arg.strip_prefix("file://").unwrap_or(&abs_arg);
                let dest = file.replace(HOME.as_str(), "/home/antimony");
                match operation {
                    FileMode::ReadOnly => args.handle.args_i(["--ro-bind", file, &dest])?,
                    FileMode::ReadWrite => args.handle.args_i(["--bind", file, &dest])?,
                    FileMode::Executable => {
                        let contents = as_real!(fs::read_to_string(file))??;
                        super::files::add_file(&args.handle, file, contents, FileMode::Executable)?
                    }
                };
                *arg = dest;
            }
        }

        return Ok(post_args);
    }

    Ok(Vec::new())
}
