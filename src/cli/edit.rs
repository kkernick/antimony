//! Edit profiles/features, Create New Ones, and Modify the Default.

use crate::{
    cli::{self},
    shared::{
        db::{self, Database, Table},
        env::AT_HOME,
        feature::Feature,
        profile::Profile,
    },
};
use std::fs;
use user::Mode;

#[derive(clap::Args, Default)]
pub struct Args {
    /// The object to edit.
    name: String,

    /// Target the feature set rather than the profile set.
    #[arg(long)]
    pub feature: bool,
}
impl cli::Run for Args {
    fn run(self) -> anyhow::Result<()> {
        let (table, kind) = if self.feature {
            (Table::Features, "feature")
        } else {
            (Table::Profiles, "profile")
        };

        // Dump the content to a temporary file.
        let temp = temp::Builder::new()
            .owner(Mode::Effective)
            .create::<temp::File>()?;
        let content = if let Some(user) = db::dump(&self.name, Database::User, table)? {
            user
        } else if let Some(system) = db::dump(&self.name, Database::System, table)? {
            system
        } else {
            fs::read_to_string(AT_HOME.join("config").join(format!("{kind}.toml")))?
        };

        fs::write(temp.full(), content)?;
        let modified = if self.feature {
            Feature::edit(&temp.full())?
        } else {
            Profile::edit(&temp.full())?
        };

        if modified.is_some() {
            db::store_str(
                &self.name,
                &fs::read_to_string(temp.full())?,
                Database::User,
                table,
            )?
        }
        Ok(())
    }
}
