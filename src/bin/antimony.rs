/// The main antimony binary
use antimony::cli::{Run, run::as_symlink};
use anyhow::Result;
use clap::Parser;

fn main() -> Result<()> {
    rayon::ThreadPoolBuilder::new().build_global()?;
    env_logger::init();

    if as_symlink().is_err() {
        let cli = antimony::cli::Cli::parse();
        cli.command.run()
    } else {
        Ok(())
    }
}
