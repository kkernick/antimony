use clap::CommandFactory;
use clap_complete::{generate, shells};
use std::{fs, io::Error, path::Path};

fn main() -> Result<(), Error> {
    let mut cli = antimony::cli::Cli::command();
    let path = Path::new("completions");
    if !path.exists() {
        fs::create_dir(path)?;
    }

    let mut out = fs::File::create(path.join("antimony.bash"))?;
    generate(shells::Bash, &mut cli, "antimony", &mut out);

    let mut out = fs::File::create(path.join("antimony.fish"))?;
    generate(shells::Fish, &mut cli, "antimony", &mut out);

    let mut out = fs::File::create(path.join("_antimony"))?;
    generate(shells::Zsh, &mut cli, "antimony", &mut out);

    Ok(())
}
