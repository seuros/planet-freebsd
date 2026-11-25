//! State → the static site: `index.html`, `status.html`, `atom.xml`, `opml.xml`.

use std::path::Path;

use anyhow::Context;
use askama::Template;
use jiff::{SignedDuration, Timestamp};
use rama::http::protocols::rss::{
    AtomCategory, AtomEntry, AtomFeed, AtomGenerator, AtomLink, AtomPerson, AtomSource, AtomText,
    AtomTextKind,
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

const DISTROS: &[&str] =
    &["dragonflybsd", "freebsd", "ghostbsd", "hardenedbsd", "midnightbsd", "netbsd", "openbsd"];

fn distro(tag: &str) -> Option<&'static str> {
    let tag = tag.trim();
    if tag.eq_ignore_ascii_case("dragonfly-bsd") {
        return Some("dragonflybsd");
    }
    DISTROS.iter().copied().find(|name| name.eq_ignore_ascii_case(tag))
}

fn topics<'a>(source: &'a FeedSource, post: &'a Post) -> impl Iterator<Item = &'static str> + 'a {
    source.tags.iter().chain(&post.categories).filter_map(|tag| distro(tag))
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

impl Post {
    fn display_author(&self) -> Option<&str> {
        self.author.as_deref().map(str::trim).filter(|author| {
            let name = author.split_once('<').map_or(*author, |(name, _)| name.trim_end());
            !author.is_empty() && !name.eq_ignore_ascii_case("undefined")
        })
    }
}

/// The posts that make the page, newest first: per feed the newest few inside
/// the age window, muted feeds left out, capped overall.
fn select<'a>(
    config: &'a Config,
    state: &'a State,
    now: Timestamp,
    topic: Option<&str>,
) -> Vec<Entry<'a>> {
    let site = &config.site;
    let window = SignedDuration::from_hours(i64::from(site.max_age_days) * 24);
    let cutoff = now.checked_sub(window).unwrap_or(Timestamp::MIN);

    let mut entries = Vec::new();
    for source in &config.feeds {
        let Some(feed) = state.feed(source).filter(|feed| !site.mutes(feed)) else {
            continue;
        };
        let mut posts: Vec<&Post> = feed
            .posts
            .iter()
            .filter(|post| post.published >= cutoff)
            .filter(|post| topic.is_none_or(|tag| topics(source, post).any(|t| t == tag)))
            .filter(|post| {
                source.require_categories.is_empty()
                    || post.categories.iter().any(|category| {
                        source
                            .require_categories
                            .iter()
                            .any(|tag| category.trim().eq_ignore_ascii_case(tag.trim()))
                    })
            })
            .collect();
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
    revised: Option<String>,
    topics: std::collections::BTreeSet<&'static str>,
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
    root: &'static str,
    topic: Option<&'a str>,
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
#[template(path = "subscribe.html")]
struct SubscribePage<'a> {
    root: &'static str,
    site: &'a Site,
    home: &'a str,
    generated: &'a str,
}

#[derive(Template)]
#[template(path = "status.html")]
struct StatusPage<'a> {
    root: &'static str,
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
            revised: entry.post.revised.map(minute),
            topics: topics(entry.source, entry.post).collect(),
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

fn atom_entry(entry: &Entry<'_>, distro_scheme: &str, source_scheme: &str) -> Option<AtomEntry> {
    let post = entry.post;
    let link = Uri::parse(post.link.as_str()).ok()?;
    // RSS guids are free text; Atom ids must be IRIs. The link is one.
    let id = Uri::parse(post.id.as_str()).unwrap_or_else(|_| link.clone());
    let feed_name = &entry.source.name;
    let mut item =
        AtomEntry::new(id, html(&post.title_html), post.revised.unwrap_or(post.published))
            .with_author(AtomPerson::new(post.display_author().unwrap_or(feed_name)))
            .with_link(AtomLink::alternate(link))
            .with_summary(html(&post.excerpt_html))
            .with_published(post.published);
    item.source = Some(AtomSource {
        id: Uri::parse(entry.source.url.as_str()).ok(),
        title: Some(AtomText::text(feed_name.as_str())),
        updated: None,
    });
    let source_tags = entry
        .source
        .tags
        .iter()
        .map(|tag| tag.trim())
        .filter(|tag| !tag.is_empty() && distro(tag).is_none());
    let labels: std::collections::BTreeSet<_> = topics(entry.source, post)
        .map(|tag| (distro_scheme, tag))
        .chain(source_tags.map(|tag| (source_scheme, tag)))
        .collect();
    item.categories = labels
        .into_iter()
        .map(|(scheme, term)| AtomCategory {
            term: term.into(),
            scheme: Some(scheme.into()),
            label: None,
        })
        .collect();
    Some(item)
}

async fn atom(
    site: &Site,
    entries: &[Entry<'_>],
    now: Timestamp,
    topic: Option<&str>,
) -> anyhow::Result<Vec<u8>> {
    let home = site.url.trim_end_matches('/');
    let distro_scheme = format!("{home}/taxonomy/distro");
    let source_scheme = format!("{home}/taxonomy/source-type");
    let (home, title) = topic.map_or_else(
        || (home.to_owned(), site.title.clone()),
        |tag| (format!("{home}/tags/{tag}"), format!("{} · +{tag}", site.title)),
    );
    let root = Uri::parse(format!("{home}/"))?;
    let feed = AtomFeed::builder()
        .id(root.clone())
        .title(AtomText::text(title))
        .updated(
            entries
                .iter()
                .map(|entry| entry.post.revised.unwrap_or(entry.post.published))
                .max()
                .unwrap_or(now),
        )
        .with_link(AtomLink::self_link(Uri::parse(format!("{home}/atom.xml"))?))
        .with_link(AtomLink::alternate(root))
        .with_generator(AtomGenerator::new("planet-freebsd"))
        .with_entries(
            entries.iter().filter_map(|entry| atom_entry(entry, &distro_scheme, &source_scheme)),
        )
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
    let generated = minute(now);
    write_atomic(&out.join("atom.css"), include_bytes!("../templates/atom.css"))?;
    let mut count = 0;
    for topic in std::iter::once(None).chain(DISTROS.iter().copied().map(Some)) {
        let entries = select(config, state, now, topic);
        let dir = topic.map_or_else(|| out.to_owned(), |tag| out.join("tags").join(tag));
        let root = if topic.is_some() { "../../" } else { "" };
        if topic.is_none() {
            count = entries.len();
        }
        let index = IndexPage {
            root,
            topic,
            site,
            generated: generated.clone(),
            days: days(&entries),
            feeds: &config.feeds,
        };
        write_page(&dir, "index.html", &index)?;
        let mut feed = atom(site, &entries, now, topic).await.context("rendering atom.xml")?;
        // A stylesheet instruction belongs after the XML declaration, before the root.
        let start = if feed.starts_with(b"<?xml ") {
            feed.windows(2).position(|pair| pair == b"?>").context("incomplete XML declaration")?
                + 2
        } else {
            0
        };
        let style = format!("\n<?xml-stylesheet type=\"text/css\" href=\"{root}atom.css\"?>\n");
        feed.splice(start..start, style.bytes());
        write_atomic(&dir.join("atom.xml"), &feed)?;
    }
    write_page(
        out,
        "subscribe.html",
        &SubscribePage {
            root: "",
            site,
            home: site.url.trim_end_matches('/'),
            generated: &generated,
        },
    )?;
    let status = StatusPage { root: "", site, generated, rows: rows(config, state) };
    let opml = Opml { site, feeds: &config.feeds };

    write_page(out, "status.html", &status)?;
    write_page(out, "opml.xml", &opml)?;
    Ok(count)
}

#[cfg(test)]
mod tests;
