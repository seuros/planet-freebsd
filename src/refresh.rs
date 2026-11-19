//! One polling pass over every configured feed.

use std::collections::BTreeMap;

use jiff::Timestamp;
use rama::futures::{StreamExt, stream};
use rama::http::client::DefaultHttpWebClient;

use crate::config::{Config, FeedSource, Site};
use crate::fetch::{Fetched, Validators, fetch_with_retry};
use crate::ingest;
use crate::state::{FeedState, State};

/// Feeds downloaded at once. Each is a different host, so this is about our
/// own sockets, not anyone's rate limit.
const CONCURRENCY: usize = 8;

/// Poll every feed in `config` and fold the results into `state`.
///
/// A feed that fails keeps its cached posts and gains a failure; a feed that
/// answers (200 or 304) is healthy again. Feeds no longer in the config are
/// forgotten. `force` skips the conditional GET, for when `filter` or
/// `excerpt_chars` changed and the cached posts must be rebuilt.
pub async fn refresh(config: &Config, state: &mut State, now: Timestamp, force: bool) {
    let client = DefaultHttpWebClient::default();
    let user_agent = format!("planet-freebsd/{} (+{})", env!("CARGO_PKG_VERSION"), config.site.url);

    let jobs = config.feeds.iter().map(|source| {
        let previous = state.feed(source).cloned().unwrap_or_default();
        let (client, user_agent, site) = (&client, user_agent.as_str(), &config.site);
        async move {
            let feed = refresh_one(client, user_agent, source, site, previous, now, force).await;
            (source.url.clone(), feed)
        }
    });
    let feeds: BTreeMap<String, FeedState> =
        stream::iter(jobs).buffer_unordered(CONCURRENCY).collect().await;
    state.feeds = feeds;
}

async fn refresh_one(
    client: &DefaultHttpWebClient,
    user_agent: &str,
    source: &FeedSource,
    site: &Site,
    mut feed: FeedState,
    now: Timestamp,
    force: bool,
) -> FeedState {
    let known = if force {
        Validators::default()
    } else {
        Validators { etag: feed.etag.clone(), last_modified: feed.last_modified.clone() }
    };

    let outcome = match fetch_with_retry(client, user_agent, &source.url, &known).await {
        Ok(Fetched::NotModified) => Ok(()),
        Ok(Fetched::Fresh { body, validators }) => {
            match ingest::parse(body, source, site, &feed.posts, now).await {
                Ok(posts) => {
                    feed.posts = posts;
                    feed.etag = validators.etag;
                    feed.last_modified = validators.last_modified;
                    Ok(())
                }
                Err(error) => Err(format!("not a readable feed: {error}")),
            }
        }
        Err(error) => Err(error.to_string()),
    };

    match outcome {
        Ok(()) => {
            feed.failures = 0;
            feed.last_error = None;
            feed.last_success = Some(now);
        }
        Err(error) => {
            feed.failures = feed.failures.saturating_add(1);
            feed.last_error = Some(error);
        }
    }
    feed
}
