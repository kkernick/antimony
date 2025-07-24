use clap::CommandFactory;
use clap_complete::{generate, shells};
use std::io::Error;

fn main() -> Result<(), Error> {
    let mut cli = antimony::cli::Cli::command();

    let mut out = std::fs::File::create("antimony.bash")?;
    generate(shells::Bash, &mut cli, "antimony", &mut out);

    let mut out = std::fs::File::create("antimony.fish")?;
    generate(shells::Fish, &mut cli, "antimony", &mut out);

    let mut out = std::fs::File::create("_antimony")?;
    generate(shells::Zsh, &mut cli, "antimony", &mut out);

    Ok(())
}
