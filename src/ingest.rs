//! A downloaded feed document → the posts the planet keeps.

use std::collections::BTreeSet;

use jiff::Timestamp;
use rama::http::Body;
use rama::http::protocols::rss::{AtomTextKind, Feed, FeedItem, FeedParseError, ItemIdView};

use crate::config::{FeedSource, Site, is_http};
use crate::state::Post;
use crate::text;

/// Length of a headline made from the body of a title-less item.
const HEADLINE_CHARS: usize = 100;

/// Parse an RSS 2.0 or Atom 1.0 document and keep the posts that have an
/// http(s) link and pass the feed's `filter`.
///
/// `previous` is what the last run kept for this feed: a post seen before
/// keeps its `first_seen`, which is also its date when the publisher gives
/// none or one in the future.
///
/// # Errors
/// The document is not a feed in either format.
pub async fn parse(
    body: String,
    source: &FeedSource,
    site: &Site,
    previous: &[Post],
    now: Timestamp,
) -> Result<Vec<Post>, FeedParseError> {
    let items: Vec<FeedItem> = match Feed::from_body(Body::from(body)).await? {
        Feed::Rss2(feed) => feed.items.into_iter().map(FeedItem::from).collect(),
        Feed::Atom(feed) => feed.entries.into_iter().map(FeedItem::from).collect(),
    };

    let mut seen = BTreeSet::new();
    let mut posts = Vec::new();
    for item in &items {
        let Some(post) = post_from(item, site, previous, now) else { continue };
        if matches_filter(&post, &source.filter) && seen.insert(post.id.clone()) {
            posts.push(post);
        }
    }
    Ok(posts)
}

fn post_from(item: &FeedItem, site: &Site, previous: &[Post], now: Timestamp) -> Option<Post> {
    // The link becomes an href on our origin: `javascript:` and friends stop here.
    let link = item.link().map(|uri| uri.as_str().into_owned()).filter(|link| is_http(link))?;
    let id = match item.id() {
        Some(ItemIdView::Rss2Guid(guid)) if !guid.trim().is_empty() => guid.to_string(),
        Some(ItemIdView::AtomId(id)) => id.as_str().into_owned(),
        _ => link.clone(),
    };

    let excerpt = text::strip(item.summary().or_else(|| item.content()).unwrap_or_default());
    let title_html = match item {
        FeedItem::Atom(entry) if entry.title.kind != AtomTextKind::Text => {
            text::strip(&entry.title.value)
        }
        _ => text::escape(item.title().unwrap_or_default().trim()),
    };
    // Title-less items (news tickers, microblogs) are headed by their own text.
    let (title_html, excerpt) = if title_html.is_empty() {
        let headline = text::truncate(&excerpt, HEADLINE_CHARS);
        let rest = if headline == excerpt { String::new() } else { excerpt };
        (if headline.is_empty() { "(untitled)".to_string() } else { headline }, rest)
    } else {
        (title_html, excerpt)
    };

    let prior = previous.iter().find(|post| post.id == id);
    let first_seen = prior.map_or(now, |post| post.first_seen);
    let reported = item.published().filter(|date| *date <= now);
    let confirmed =
        prior.filter(|post| post.publication_confirmed).map(|post| post.published).or(reported);
    let published = confirmed.unwrap_or(first_seen);
    // RSS has no item-level updated field, but publishers may advance pubDate
    // on an existing GUID. Never use the poll time as evidence of a correction.
    let revised = item
        .updated()
        .or(reported)
        .filter(|date| *date > published && *date <= now)
        .max(prior.and_then(|post| post.revised));

    Some(Post {
        id,
        title_html,
        link,
        author: item
            .authors()
            .next()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string),
        published,
        publication_confirmed: confirmed.is_some(),
        revised,
        first_seen,
        excerpt_html: text::truncate(&excerpt, site.excerpt_chars),
        categories: item.categories().map(str::to_string).collect(),
    })
}

fn matches_filter(post: &Post, filter: &[String]) -> bool {
    if filter.is_empty() {
        return true;
    }
    let haystack =
        format!("{} {} {}", post.title_html, post.categories.join(" "), post.excerpt_html)
            .to_lowercase();
    filter.iter().any(|word| haystack.contains(&word.to_lowercase()))
}

#[cfg(test)]
mod tests;
