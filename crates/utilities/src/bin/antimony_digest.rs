use std::{fs, path::Path};

use antimony::shared::{
    db::{self, Database, Table},
    feature::Feature,
    profile::Profile,
};
use nix::unistd::chdir;
use spawn::Spawner;

fn main() -> anyhow::Result<()> {
    let root = Spawner::new("git")?
        .args(["rev-parse", "--show-toplevel"])?
        .output(spawn::StreamMode::Pipe)
        .spawn()?
        .output_all()?;
    let root = &root[..root.len() - 1];
    chdir(root)?;

    let out = Path::new(root).join("config");
    unsafe { std::env::set_var("AT_HOME", out.to_string_lossy().into_owned()) };

    let profiles = Path::new(root).join("config").join("profiles");
    for profile in fs::read_dir(profiles)? {
        let profile = profile?;
        let toml = toml::from_str::<Profile>(&fs::read_to_string(profile.path())?)?;
        if let Some(name) = profile.file_name().to_string_lossy().strip_suffix(".toml") {
            db::save(name, &toml, Database::System, Table::Profiles)?;
        }
    }

    let features = Path::new(root).join("config").join("features");
    for feature in fs::read_dir(features)? {
        let feature = feature?;
        let toml = toml::from_str::<Feature>(&fs::read_to_string(feature.path())?)?;
        if let Some(name) = feature.file_name().to_string_lossy().strip_suffix(".toml") {
            db::save(name, &toml, Database::System, Table::Features)?;
        }
    }

    db::store_str(
        "default",
        &fs::read_to_string(Path::new(root).join("config").join("default.toml"))?,
        Database::System,
        Table::Profiles,
    )?;

    db::execute(Database::System, |db| {
        db.pragma_update(None, "wal_checkpoint", "TRUNCATE")?;
        Ok(())
    })?;
    Ok(())
}
