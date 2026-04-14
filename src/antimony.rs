//! The main Antimony executable.

use antimony::{
    cli::{Run, run::as_symlink},
    shared::{self, config::CONFIG_FILE},
};
use anyhow::Result;
use clap::Parser;
use rayon::ThreadPoolBuilder;
use std::thread::available_parallelism;

fn main() -> Result<()> {
    for (key, value) in CONFIG_FILE.environment() {
        if std::env::var(key).is_err() {
            unsafe { std::env::set_var(key, value) }
        }
    }

    notify::init()?;
    notify::set_notifier(Box::new(shared::logger))?;

    // Somehow, using half the available parallel drastically improves performance.
    // However, 3 causes a massive regression.
    ThreadPoolBuilder::new()
        .num_threads(available_parallelism()?.get() / 2)
        .build_global()?;

    // In most SetUID applications, The effective user is the privileged
    // one (Usually root), but in Antimony its the opposite. The user
    // is considered privileged, as the antimony user has no permission
    // besides its own folder.
    //
    // Though we don't drop privilege within the main antimony application,
    // instead dropping when executing the sandbox/helpers, this codebase
    // treats swapping to the user as a privileged operation, and operates
    // by default under the assumption we are running under antimony.
    //
    // This is not a security consideration, just a practical one.
    user::set(user::Mode::Effective)?;

    if as_symlink().is_err() {
        let cli = antimony::cli::Cli::parse();
        cli.command.run()
    } else {
        Ok(())
    }
}
