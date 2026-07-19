//! Generate shell completions for Antimony.
#![allow(unused_crate_dependencies)]

use antimony::shared::{
    feature::Feature,
    profile::Profile,
    store::{Object, SYSTEM_STORE},
};

fn main() -> anyhow::Result<()> {
    SYSTEM_STORE
        .borrow()
        .get(Object::Profile)?
        .iter()
        .try_for_each(|name| -> anyhow::Result<()> {
            let profile = SYSTEM_STORE
                .borrow()
                .fetch(name.as_str(), Object::Profile)?;
            let string = toml::to_string(&toml::from_str::<Profile>(&profile)?)?;
            SYSTEM_STORE
                .borrow()
                .store(name, Object::Profile, &string)?;
            Ok(())
        })?;

    SYSTEM_STORE
        .borrow()
        .get(Object::Feature)?
        .iter()
        .try_for_each(|name| -> anyhow::Result<()> {
            let profile = SYSTEM_STORE
                .borrow()
                .fetch(name.as_str(), Object::Feature)?;
            let string = toml::to_string(&toml::from_str::<Feature>(&profile)?)?;
            SYSTEM_STORE
                .borrow()
                .store(name, Object::Feature, &string)?;
            Ok(())
        })?;
    Ok(())
}
