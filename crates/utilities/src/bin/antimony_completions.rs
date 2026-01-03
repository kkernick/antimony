//! Generate shell completions for Antimony.

use clap::CommandFactory;
use clap_complete::{generate, shells};
use spawn::Spawner;
use std::{fs, path::Path};

fn main() -> anyhow::Result<()> {
    let mut cli = antimony::cli::Cli::command();

    let root = Spawner::new("git")?
        .args(["rev-parse", "--show-toplevel"])?
        .output(spawn::StreamMode::Pipe)
        .spawn()?
        .output_all()?;
    let root = &root[..root.len() - 1];

    let path = Path::new(root).join("completions");
    if !path.exists() {
        fs::create_dir(&path)?;
    }

    let mut out = fs::File::create(path.join("antimony.bash"))?;
    generate(shells::Bash, &mut cli, "antimony", &mut out);

    let mut out = fs::File::create(path.join("antimony.fish"))?;
    generate(shells::Fish, &mut cli, "antimony", &mut out);

    let mut out = fs::File::create(path.join("_antimony"))?;
    generate(shells::Zsh, &mut cli, "antimony", &mut out);

    Ok(())
}
