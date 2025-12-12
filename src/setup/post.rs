use crate::shared::{env::HOME, profile::FileMode};
use anyhow::Result;
use log::debug;
use std::{borrow::Cow, fs, path::Path};
use url::Url;
use user::{self, try_run_as};

pub fn setup(args: &mut super::Args) -> Result<Vec<String>> {
    debug!("Setting up post arguments");
    let mut post_args = Vec::new();

    if let Some(mut arguments) = args.profile.arguments.take() {
        post_args.append(&mut arguments);
    }
    if let Some(mut passthrough) = args.args.passthrough.take() {
        post_args.append(&mut passthrough);
    }

    if !post_args.is_empty() {
        let operation = match args.profile.files.take() {
            Some(mut files) => match files.passthrough.take() {
                Some(passthrough) => passthrough,
                None => FileMode::ReadOnly,
            },
            None => FileMode::ReadOnly,
        };

        try_run_as!(user::Mode::Real, Result<()>, {
            for arg in &mut post_args {
                if Path::new(arg).exists() || arg.starts_with("file://") {
                    debug!("File passthrough: {arg}");
                    let file = if arg.starts_with("file://") {
                        Cow::Owned(
                            Url::parse(arg)?
                                .to_file_path()
                                .map_err(|_| anyhow::Error::msg("Malformed URI"))?
                                .to_string_lossy()
                                .into_owned(),
                        )
                    } else {
                        Cow::Borrowed(arg.as_str())
                    };

                    let dest = arg.replace(HOME.as_str(), "/home/antimony");
                    match operation {
                        FileMode::ReadOnly => args.handle.args_i(["--ro-bind", &file, &dest])?,
                        FileMode::ReadWrite => args.handle.args_i(["--bind", &file, &dest])?,
                        FileMode::Executable => {
                            let contents = fs::read_to_string(file.as_ref())?;
                            super::files::add_file(
                                &args.handle,
                                &file,
                                contents,
                                FileMode::Executable,
                            )?
                        }
                    };
                    *arg = dest;
                }
            }
            Ok(())
        })?;

        return Ok(post_args);
    }

    Ok(Vec::new())
}
