//! `planet.toml`: site settings and the feed list.

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{Context, bail};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub site: Site,
    #[serde(rename = "feed", default)]
    pub feeds: Vec<FeedSource>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Site {
    pub title: String,
    /// Public URL of the planet, without a trailing slash.
    pub url: String,
    #[serde(default)]
    pub description: String,
    /// Posts on the page and in the combined feed.
    #[serde(default = "default_max_posts")]
    pub max_posts: usize,
    /// Newest posts kept per feed, so one busy feed cannot fill the page.
    #[serde(default = "default_max_posts_per_feed")]
    pub max_posts_per_feed: usize,
    #[serde(default = "default_max_age_days")]
    pub max_age_days: u32,
    #[serde(default = "default_excerpt_chars")]
    pub excerpt_chars: usize,
    /// Consecutive failed runs before a feed's cached posts are hidden.
    #[serde(default = "default_mute_after_failures")]
    pub mute_after_failures: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeedSource {
    pub name: String,
    pub url: String,
    /// The human-facing site; the feed URL stands in when absent.
    pub site: Option<String>,
    /// Curated source labels, independent of the post filter.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Show only posts carrying one of these publisher categories (exact, case-insensitive).
    #[serde(default)]
    pub require_categories: Vec<String>,
    /// Keep only posts mentioning one of these words (title, categories or
    /// excerpt, case-insensitive). Empty keeps everything.
    #[serde(default)]
    pub filter: Vec<String>,
}

const fn default_max_posts() -> usize {
    60
}
const fn default_max_posts_per_feed() -> usize {
    5
}
const fn default_max_age_days() -> u32 {
    365
}
const fn default_excerpt_chars() -> usize {
    500
}
const fn default_mute_after_failures() -> u32 {
    5
}

pub fn is_http(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://")
}

impl Config {
    /// # Errors
    /// The file is unreadable, is not valid TOML for this schema, or fails
    /// [`Config::problems`].
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let config = Self::parse(&text).with_context(|| format!("parsing {}", path.display()))?;
        let problems = config.problems();
        if !problems.is_empty() {
            bail!("{}:\n  {}", path.display(), problems.join("\n  "));
        }
        Ok(config)
    }

    /// # Errors
    /// `text` is not valid TOML for this schema.
    pub fn parse(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }

    /// Everything wrong with an otherwise well-formed config, one line each.
    pub fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if !is_http(&self.site.url) {
            problems.push(format!("site.url `{}` is not an http(s) URL", self.site.url));
        }
        if self.feeds.is_empty() {
            problems.push("no [[feed]] entries".to_string());
        }
        let mut seen = BTreeSet::new();
        for feed in &self.feeds {
            if feed.name.trim().is_empty() {
                problems.push(format!("feed `{}` has an empty name", feed.url));
            }
            if !is_http(&feed.url) {
                problems.push(format!(
                    "feed `{}`: url `{}` is not an http(s) URL",
                    feed.name, feed.url
                ));
            }
            if feed.site.as_deref().is_some_and(|site| !is_http(site)) {
                problems.push(format!("feed `{}`: site is not an http(s) URL", feed.name));
            }
            if !seen.insert(feed.url.as_str()) {
                problems.push(format!("feed url `{}` is listed twice", feed.url));
            }
        }
        problems
    }
}

#[cfg(test)]
mod tests;
