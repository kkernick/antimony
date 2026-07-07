//! Antimony's custom file-finder, crawling directories and indexing the results.

use crate::{
    fab::{Cache, get_cache, ldd, lib::ROOTS, write_cache},
    shared::{Map, Set, store::Object},
    timer,
};
use anyhow::Result;
use bilrost::{Enumeration, Message};
use rayon::prelude::*;
use std::{borrow::Cow, fs, path::Path};

/// Each directory is indexed based on filetype, and a list of those files.
pub type DirMap = Map<DirType, Set<String>>;

/// A Bilrost-compatible parcel for serializing a `DirMap`.
#[derive(Default, Debug, Message)]
struct DirMessage {
    /// The actual `DirMap`
    map: DirMap,
}

/// The kind of entry in a `DirMap`.
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

pub fn crawl_dir(dir: &str) -> Result<DirMap> {
    timer!(
        "::crawl_dir",
        if let Some(dir) = get_cache::<DirMessage>(dir, Object::Search)? {
            Ok(dir.map)
        } else {
            let mut crawled: DirMap = Map::default();
            let entries = fs::read_dir(dir)?
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
                .collect_vec_list();
            for result in entries {
                for (t, p) in result {
                    if t.is_file() {
                        crawled.entry(DirType::File).or_default().insert(p);
                    } else if t.is_dir() {
                        crawled.entry(DirType::Dir).or_default().insert(p);
                    } else if t.is_symlink() {
                        crawled.entry(DirType::Link).or_default().insert(p);
                    }
                }
            }
            Ok(write_cache(dir, DirMessage { map: crawled }, Object::Search)?.map)
        }
    )
}

/// Recursive Crawler Runner.
fn crawler(dir: &str, max: usize, depth: usize) -> Result<DirMap> {
    if depth >= max {
        Ok(DirMap::default())
    } else {
        let mut crawled = crawl_dir(dir)?;
        if let Some(dirs) = crawled.get(&DirType::Dir) {
            let dir_crawl = dirs
                .into_par_iter()
                .filter_map(|d| crawler(d, max, depth.saturating_add(1)).ok())
                .collect_vec_list();

            for result in dir_crawl {
                for mut crawl in result {
                    if let Some(files) = crawl.remove(&DirType::File) {
                        crawled.entry(DirType::File).or_default().extend(files);
                    }
                    if let Some(dir) = crawl.remove(&DirType::Dir) {
                        crawled.entry(DirType::Dir).or_default().extend(dir);
                    }
                    if let Some(link) = crawl.remove(&DirType::Link) {
                        crawled.entry(DirType::Link).or_default().extend(link);
                    }
                }
            }
        }
        Ok(crawled)
    }
}

/// Recursive Crawl a Directory, such that all directories up to `max_depth`
/// are crawled on top of the root.
#[inline]
pub fn recursive_crawl(dir: &str, max_depth: Option<usize>) -> Result<DirMap> {
    timer!(
        "::recursive_crawl",
        crawler(dir, max_depth.unwrap_or(usize::MAX), 0)
    )
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
#[inline]
pub fn match_wildcard(wild: &str, name: &str) -> bool {
    timer!("::match_wildcard", {
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
    })
}

/// Get all executable files in a directory. This is very expensive.
pub fn dir(dir: &str) -> Result<Set<String>> {
    timer!("::find::dir", {
        if let Ok(Some(libraries)) = get_cache::<Cache>(dir, Object::Directories) {
            return Ok(libraries.cache.into_iter().collect());
        }
        let crawled = recursive_crawl(dir, None)?
            .remove(&DirType::File)
            .unwrap_or_default()
            .into_par_iter()
            .filter_map(|lib| ldd(&lib).ok())
            .flatten()
            .collect();
        Ok(write_cache(dir, Cache { cache: crawled }, Object::Directories)?.cache)
    })
}

/// Find all matches in a directory. We only match the top level for performance considerations.
#[allow(
    clippy::unwrap_used,
    clippy::missing_panics_doc,
    reason = "Both unwraps are done with explicit knowledge that they cannot fail."
)]
pub fn wildcards(pattern: &str, lib: bool, filter: WildcardFilter) -> Result<Set<String>> {
    timer!("::find::wildcards", {
        let run = |mut dir: Cow<'_, str>, mut base: &str| -> Result<Set<String>> {
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
                .collect())
        };

        if let Some(libraries) = get_cache::<Cache>(pattern, Object::Wildcards)? {
            return Ok(libraries.cache);
        }

        let libraries = if lib {
            ROOTS
                .par_iter()
                .filter_map(|root| run(Cow::Borrowed(&root), pattern).ok())
                .flatten()
                .collect()
        } else {
            run(Cow::Borrowed("/usr/bin"), pattern)?
        };

        Ok(write_cache(pattern, Cache { cache: libraries }, Object::Wildcards)?.cache)
    })
}
