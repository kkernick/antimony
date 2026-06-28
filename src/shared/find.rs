//! Antimony's custom file-finder, crawling directories and indexing the results.

use crate::{
    fab::{Cache, get_cache, lib::ROOTS, write_cache},
    shared::store::Object,
};
use crate::{
    shared::{Map, Set},
    timer,
};
use anyhow::Result;
use bilrost::{Enumeration, Message};
use log::trace;
use rayon::prelude::*;
use std::path::Path;
use std::{borrow::Cow, fs};

/// Each directory is indexed based on file type, and a list of those files.
pub type DirMap = Map<DirType, Set<String>>;

/// A Bilrost-compatible parcel for serializing a `DirMap`.
#[derive(Default, Debug, Message)]
struct DirMessage {
    /// The actual `DirMap`
    map: DirMap,
}

/// The kind of enty in a `DirMap`.
#[derive(PartialEq, Eq, Hash, Debug, Enumeration)]
pub enum DirType {
    /// Files
    File = 0,

    /// Directories
    Dir = 1,

    /// Symlinks
    Link = 2,
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum WildcardFilter {
    Files,
    Directories,
}
impl WildcardFilter {
    #[must_use]
    pub const fn find_filter(&self) -> &'static str {
        match self {
            Self::Files => "f,l",
            Self::Directories => "d",
        }
    }
}

pub fn crawl_dir(dir: &str) -> Result<DirMap> {
    if let Some(dir) = get_cache::<DirMessage>(dir, Object::Search)? {
        Ok(dir.map)
    } else {
        trace!("Crawling {dir}");
        let mut crawled: DirMap = Map::default();
        let entries: Vec<_> = fs::read_dir(dir)?
            .par_bridge()
            .into_par_iter()
            .filter_map(|e| {
                if let Ok(e) = e
                    && let Ok(t) = e.file_type()
                {
                    Some((t, e.path().to_string_lossy().into_owned()))
                } else {
                    None
                }
            })
            .collect();
        for (t, p) in entries {
            if t.is_file() {
                crawled.entry(DirType::File).or_default().insert(p);
            } else if t.is_dir() {
                crawled.entry(DirType::Dir).or_default().insert(p);
            } else if t.is_symlink() {
                crawled.entry(DirType::Link).or_default().insert(p);
            }
        }
        Ok(write_cache(dir, DirMessage { map: crawled }, Object::Search)?.map)
    }
}

/// Recursive Crawler Runner.
fn crawler(dir: &str, max: usize, depth: usize) -> Result<DirMap> {
    if depth >= max {
        Ok(DirMap::default())
    } else {
        let mut crawled = crawl_dir(dir)?;
        let dirs = crawled
            .get(&DirType::Dir)
            .map_or_else(Set::default, Clone::clone);

        let dir_crawl: Vec<_> = dirs
            .into_par_iter()
            .filter_map(|d| crawler(&d, max, depth.saturating_add(1)).ok())
            .collect();
        for mut crawl in dir_crawl {
            if let Some(files) = crawl.remove(&DirType::File) {
                crawled.entry(DirType::File).or_default().extend(files);
            }
            if let Some(files) = crawl.remove(&DirType::Dir) {
                crawled.entry(DirType::Dir).or_default().extend(files);
            }
            if let Some(files) = crawl.remove(&DirType::Link) {
                crawled.entry(DirType::Link).or_default().extend(files);
            }
        }
        Ok(crawled)
    }
}

/// Recursive Crawl a Directory, such that all directories up to `max_depth`
/// are crawled on top of the root.
pub fn recursive_crawl(dir: &str, max_depth: Option<usize>) -> Result<DirMap> {
    crawler(dir, max_depth.unwrap_or(usize::MAX), 0)
}

/// Check if a given wildcard pattern matches a candidate.
/// This only matches four forms of wildcard:
/// 1. Prefix: "*test" matches "atest"
/// 2. Suffix: "libtest*"" matches "libtest.so"
/// 3. Inner: "*test*" matches "this is a test"
/// 4. Outer: "te*st" matches "tea test"
/// 5. Exact: "test" matches "test"
///
/// ## Examples
///
/// ```rust
/// use antimony::shared::find::match_wildcard;
/// assert!(match_wildcard("*test", "atest"));
/// assert!(match_wildcard("libtest*", "libtest.so"));
/// assert!(match_wildcard("*test*", "this is a test"));
/// assert!(match_wildcard("test", "test"));
/// assert!(match_wildcard("te*st", "tea test"));
/// ```
#[allow(clippy::option_if_let_else, reason = "That is horrendous")]
#[must_use]
pub fn match_wildcard(wild: &str, name: &str) -> bool {
    let name = if let Some(i) = name.rfind('/')
        && let Some(i) = i.checked_add(1)
    {
        &name[i..]
    } else {
        name
    };

    if let Some(suffix) = wild.strip_prefix('*') {
        if let Some(inner) = suffix.strip_suffix('*') {
            name.contains(inner)
        } else {
            name.ends_with(suffix)
        }
    } else if let Some(prefix) = wild.strip_suffix('*') {
        name.starts_with(prefix)
    } else if let Some((prefix, suffix)) = wild.split_once('*') {
        name.starts_with(prefix) && name.ends_with(suffix)
    } else {
        wild == name
    }
}

/// Find all matches in a directory. We only match the top level for performance considerations.
///
/// ## Examples
///
/// ```rust
/// use antimony::fab::{get_wildcards,lib::WildcardFilter};
/// get_wildcards("glib*", true, WildcardFilter::Files).expect("Failed to find Glib");
/// ```
#[allow(
    clippy::unwrap_used,
    clippy::missing_panics_doc,
    reason = "Both unwraps are done with explicit knowledge that they cannot fail."
)]
pub fn get_wildcards(
    pattern: &str,
    lib: bool,
    filter: WildcardFilter,
) -> Result<Set<Cow<'static, str>>> {
    timer!("::get_wildcards", {
        let run = |mut dir: Cow<'_, str>, mut base: &str| -> Result<Set<Cow<'static, str>>> {
            if let Some(i) = base.rfind('/')
                && let Some(i) = i.checked_add(1)
            {
                dir = Cow::Owned(format!("{dir}/{}", &base[..i]));
                base = &base[i..];
                if !Path::new(dir.as_ref()).exists() {
                    return Ok(Set::default());
                }
            }

            let mut crawled = crawl_dir(&dir)?;
            let matches = match filter {
                WildcardFilter::Files => {
                    let mut matches = crawled.remove(&DirType::File).unwrap_or_default();
                    matches.extend(crawled.remove(&DirType::Link).unwrap_or_default());
                    matches
                }
                WildcardFilter::Directories => crawled.remove(&DirType::Dir).unwrap_or_default(),
            };

            Ok(matches
                .into_par_iter()
                .filter(|name| match_wildcard(base, name))
                .map(Cow::Owned)
                .collect())
        };

        if let Some(libraries) = get_cache::<Cache>(pattern, Object::Wildcards)? {
            return Ok(libraries.cache);
        }

        // If we have a direct path, call `find /path/to/parent -name file*`
        let libraries = if pattern.starts_with('/') {
            let i = pattern.rfind('/').unwrap();
            run(
                Cow::Borrowed(&pattern[..i]),
                &pattern[i.checked_add(1).unwrap_or(i)..],
            )?

        // If we're looking for libraries, check each library root.
        } else if lib {
            let mut libraries = Set::default();
            for root in ROOTS.iter() {
                if let Ok(lib) = run(Cow::Borrowed(&root), pattern) {
                    libraries.extend(lib);
                }
            }

            libraries
        // Otherwise, we assume we're looking for binaries.
        } else {
            run(Cow::Borrowed("/usr/bin"), pattern)?
        };

        Ok(write_cache(pattern, Cache { cache: libraries }, Object::Wildcards)?.cache)
    })
}
