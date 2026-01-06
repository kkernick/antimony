//! The main Antimony executable.

use antimony::{
    cli::{Run, run::as_symlink},
    shared::{self, db, profile},
};
use anyhow::Result;
use clap::Parser;

fn main() -> Result<()> {
    rayon::spawn(|| {
        let _ = profile::USER_CACHE.as_ref();
    });
    rayon::spawn(|| {
        let _ = profile::SYSTEM_CACHE.as_ref();
    });
    rayon::spawn(|| {
        let _ = profile::HASH_CACHE.as_ref();
    });

    rayon::spawn_broadcast(|_| {
        let _ = db::USER_DB;
        let _ = db::SYS_DB;
        let _ = db::CACHE_DB;
    });

    notify::init()?;
    notify::set_notifier(Box::new(shared::logger))?;

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
