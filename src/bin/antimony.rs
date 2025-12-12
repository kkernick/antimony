/// The main antimony binary
use antimony::cli::{Run, run::as_symlink};
use anyhow::Result;
use clap::Parser;

fn main() -> Result<()> {
    rayon::ThreadPoolBuilder::new().build_global()?;
    env_logger::init();

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
