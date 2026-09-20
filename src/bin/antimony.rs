//! The main Antimony executable.
#![allow(unused_crate_dependencies)]

use antimony::{
    cli::{self, Command, run::as_symlink},
    shared, timer,
};
use anyhow::Result;
use clap::Parser;
use rayon::ThreadPoolBuilder;
use std::thread::available_parallelism;

fn main() -> Result<()> {
    // Unfortunately, there is only a finite about of things that need
    // Effective Access (The Configuration Store, SOF), and only a handful
    // actual need write-permission.
    //
    // In contrast, there are countably infinite things that the Real user
    // needs access for. Their home, their runtime directory, temp, whatever
    // mounts they may have installed, and whatever else they may have setup.
    // We cannot account for it, and up until now the solution has been simply
    // to choke hot-loops with our thread-safe `as_real` macros. Performance
    // considerations require us to change how Antimony operates.
    user::set(user::Mode::Real)?;

    // Somehow, using half the available parallel drastically improves performance.
    // However, 3 causes a massive regression.
    timer!(
        "::thread_pool",
        ThreadPoolBuilder::new()
            .num_threads(available_parallelism()?.get() / 2)
            .build_global()
    )?;

    notify::init()?;
    notify::set_notifier(Box::new(shared::logger))?;

    let ret = if as_symlink().is_err() {
        match cli::Cli::parse().command {
            Command::Run(args) => args.run(),
            Command::Edit(args) => args.run(),
            Command::Refresh(args) => args.run(),
            Command::Integrate(args) => args.run(),
            Command::Remove(args) => args.run(),
            Command::Export(args) => args.run(),
            Command::Import(args) => args.run(),
            Command::Info(args) => args.run(),
        }
    } else {
        Ok(())
    };

    #[cfg(debug_assertions)]
    #[allow(clippy::absolute_paths)]
    {
        let mut total = std::num::Saturating(0u128);
        let mut sorted = shared::TIME_MAP
            .iter()
            .map(|r| {
                total += *r.value();
                (r.key().to_owned(), *r.value())
            })
            .collect::<Vec<_>>();
        sorted.sort_by_key(|a| a.1);
        sorted.reverse();
        for (k, v) in sorted {
            use std::ops::Div;

            #[allow(clippy::cast_precision_loss)]
            let weight = (v * std::num::Saturating(100)).div(total);
            log::info!("{k} => {v} ({weight}%)");
        }

        log::info!("TOTAL => {total}");
    }

    ret
}
