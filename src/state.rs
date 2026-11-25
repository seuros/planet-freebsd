//! What the planet remembers between runs: HTTP validators, feed health and
//! the posts last seen, keyed by feed URL.

use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::path::Path;

use anyhow::Context;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::config::FeedSource;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct State {
    #[serde(default)]
    pub feeds: BTreeMap<String, FeedState>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct FeedState {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    /// Consecutive failed runs; reset by any success.
    #[serde(default)]
    pub failures: u32,
    pub last_error: Option<String>,
    pub last_success: Option<Timestamp>,
    #[serde(default)]
    pub posts: Vec<Post>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Post {
    pub id: String,
    /// HTML-safe text, see [`crate::text`].
    pub title_html: String,
    pub link: String,
    pub author: Option<String>,
    /// The publisher's date, or `first_seen` when it is missing or in the
    /// future.
    pub published: Timestamp,
    /// Older caches did not distinguish publication from discovery.
    #[serde(default)]
    pub publication_confirmed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revised: Option<Timestamp>,
    pub first_seen: Timestamp,
    /// HTML-safe text, see [`crate::text`].
    pub excerpt_html: String,
    #[serde(default)]
    pub categories: Vec<String>,
}

impl State {
    /// What is remembered about `source`, if it was ever polled.
    pub fn feed(&self, source: &FeedSource) -> Option<&FeedState> {
        self.feeds.get(&source.url)
    }

    /// A missing file is an empty state: the first run has nothing to remember.
    ///
    /// # Errors
    /// The file exists but is unreadable or not this format.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .with_context(|| format!("parsing {}", path.display())),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error).with_context(|| format!("reading {}", path.display())),
        }
    }

    /// # Errors
    /// The file cannot be written.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        write_atomic(path, &serde_json::to_vec_pretty(self)?)
    }
}

/// Write beside the target, then rename over it: a reader (the web server, or
/// the next run) never sees half a file.
///
/// # Errors
/// The directory cannot be created or the file cannot be written.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let mut partial = path.as_os_str().to_owned();
    partial.push(".partial");
    std::fs::write(&partial, bytes).with_context(|| format!("writing {}", path.display()))?;
    std::fs::rename(&partial, path).with_context(|| format!("writing {}", path.display()))
}
