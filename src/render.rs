//! State → the static site: `index.html`, `status.html`, `atom.xml`, `opml.xml`.

use std::path::Path;

use anyhow::Context;
use askama::Template;
use jiff::{SignedDuration, Timestamp};
use rama::http::protocols::rss::{
    AtomEntry, AtomFeed, AtomGenerator, AtomLink, AtomPerson, AtomText, AtomTextKind,
};
use rama::net::uri::Uri;

use crate::config::{Config, FeedSource, Site};
use crate::state::{FeedState, Post, State, write_atomic};

/// A post together with the feed it came from.
#[derive(Debug, Clone, Copy)]
pub struct Entry<'a> {
    pub post: &'a Post,
    pub source: &'a FeedSource,
}

impl Site {
    /// `0` turns muting off.
    const fn mutes(&self, feed: &FeedState) -> bool {
        self.mute_after_failures > 0 && feed.failures >= self.mute_after_failures
    }
}

impl FeedSource {
    fn home(&self) -> &str {
        self.site.as_deref().unwrap_or(&self.url)
    }
}

/// The posts that make the page, newest first: per feed the newest few inside
/// the age window, muted feeds left out, capped overall.
pub fn select<'a>(config: &'a Config, state: &'a State, now: Timestamp) -> Vec<Entry<'a>> {
    let site = &config.site;
    let window = SignedDuration::from_hours(i64::from(site.max_age_days) * 24);
    let cutoff = now.checked_sub(window).unwrap_or(Timestamp::MIN);

    let mut entries = Vec::new();
    for source in &config.feeds {
        let Some(feed) = state.feed(source).filter(|feed| !site.mutes(feed)) else {
            continue;
        };
        let mut posts: Vec<&Post> =
            feed.posts.iter().filter(|post| post.published >= cutoff).collect();
        posts.sort_by_key(|post| std::cmp::Reverse(post.published));
        entries.extend(
            posts.into_iter().take(site.max_posts_per_feed).map(|post| Entry { post, source }),
        );
    }
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.post.published));
    entries.truncate(site.max_posts);
    entries
}

struct PostView<'a> {
    post: &'a Post,
    source: &'a FeedSource,
    time: String,
}

struct Day<'a> {
    date: String,
    posts: Vec<PostView<'a>>,
}

#[derive(Template)]
#[template(path = "index.html")]
struct IndexPage<'a> {
    site: &'a Site,
    generated: String,
    days: Vec<Day<'a>>,
    feeds: &'a [FeedSource],
}

struct FeedRow<'a> {
    source: &'a FeedSource,
    health: &'static str,
    failures: u32,
    last_success: String,
    last_error: &'a str,
    posts: usize,
}

#[derive(Template)]
#[template(path = "status.html")]
struct StatusPage<'a> {
    site: &'a Site,
    generated: String,
    rows: Vec<FeedRow<'a>>,
}

#[derive(Template)]
#[template(path = "opml.xml")]
struct Opml<'a> {
    site: &'a Site,
    feeds: &'a [FeedSource],
}

fn minute(at: Timestamp) -> String {
    at.strftime("%Y-%m-%d %H:%M UTC").to_string()
}

fn days<'a>(entries: &[Entry<'a>]) -> Vec<Day<'a>> {
    let mut days: Vec<Day<'a>> = Vec::new();
    for entry in entries {
        let date = entry.post.published.strftime("%Y-%m-%d").to_string();
        let view = PostView {
            post: entry.post,
            source: entry.source,
            time: entry.post.published.strftime("%H:%M UTC").to_string(),
        };
        match days.last_mut() {
            Some(day) if day.date == date => day.posts.push(view),
            _ => days.push(Day { date, posts: vec![view] }),
        }
    }
    days
}

fn rows<'a>(config: &'a Config, state: &'a State) -> Vec<FeedRow<'a>> {
    config
        .feeds
        .iter()
        .map(|source| {
            let feed = state.feed(source);
            let health = match feed {
                None => "pending",
                Some(feed) if config.site.mutes(feed) => "muted",
                Some(feed) if feed.failures > 0 => "failing",
                Some(_) => "ok",
            };
            FeedRow {
                source,
                health,
                failures: feed.map_or(0, |feed| feed.failures),
                last_success: feed
                    .and_then(|feed| feed.last_success)
                    .map_or_else(|| "never".to_string(), minute),
                last_error: feed.and_then(|feed| feed.last_error.as_deref()).unwrap_or_default(),
                posts: feed.map_or(0, |feed| feed.posts.len()),
            }
        })
        .collect()
}

fn html(value: &str) -> AtomText {
    AtomText { value: value.to_string(), kind: AtomTextKind::Html }
}

fn atom_entry(entry: &Entry<'_>) -> Option<AtomEntry> {
    let post = entry.post;
    let link = Uri::parse(post.link.as_str()).ok()?;
    // RSS guids are free text; Atom ids must be IRIs. The link is one.
    let id = Uri::parse(post.id.as_str()).unwrap_or_else(|_| link.clone());
    let feed_name = &entry.source.name;
    let byline = post
        .author
        .as_ref()
        .map_or_else(|| feed_name.clone(), |author| format!("{author} ({feed_name})"));
    Some(
        AtomEntry::new(id, html(&post.title_html), post.published)
            .with_author(AtomPerson::new(byline))
            .with_link(AtomLink::alternate(link))
            .with_summary(html(&post.excerpt_html))
            .with_published(post.published),
    )
}

async fn atom(site: &Site, entries: &[Entry<'_>], now: Timestamp) -> anyhow::Result<Vec<u8>> {
    let home = site.url.trim_end_matches('/');
    let root = Uri::parse(format!("{home}/"))?;
    let feed = AtomFeed::builder()
        .id(root.clone())
        .title(AtomText::text(site.title.as_str()))
        .updated(entries.first().map_or(now, |entry| entry.post.published))
        .with_link(AtomLink::self_link(Uri::parse(format!("{home}/atom.xml"))?))
        .with_link(AtomLink::alternate(root))
        .with_generator(AtomGenerator::new("planet-freebsd"))
        .with_entries(entries.iter().filter_map(atom_entry))
        .build();
    feed.to_xml().await.map_err(anyhow::Error::from_boxed)
}

fn write_page(out: &Path, name: &str, page: &impl Template) -> anyhow::Result<()> {
    let text = page.render().with_context(|| format!("rendering {name}"))?;
    write_atomic(&out.join(name), text.as_bytes())
}

/// Write the whole site into `out`, each file replaced atomically.
///
/// Returns how many posts made the page.
///
/// # Errors
/// A template fails to render, `site.url` is not a URL, or `out` is not
/// writable.
pub async fn render(
    config: &Config,
    state: &State,
    out: &Path,
    now: Timestamp,
) -> anyhow::Result<usize> {
    let site = &config.site;
    let entries = select(config, state, now);
    let generated = minute(now);

    let index = IndexPage {
        site,
        generated: generated.clone(),
        days: days(&entries),
        feeds: &config.feeds,
    };
    let status = StatusPage { site, generated, rows: rows(config, state) };
    let opml = Opml { site, feeds: &config.feeds };

    write_page(out, "index.html", &index)?;
    write_page(out, "status.html", &status)?;
    write_page(out, "opml.xml", &opml)?;
    let feed = atom(site, &entries, now).await.context("rendering atom.xml")?;
    write_atomic(&out.join("atom.xml"), &feed)?;
    Ok(entries.len())
}

#[cfg(test)]
mod tests;
