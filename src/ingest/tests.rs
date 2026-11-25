use super::*;
use crate::config::Config;

const RSS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0"><channel>
<title>A blog</title><link>https://blog.example/</link><description>words</description>
<item><title>ZFS &amp; you</title><link>https://blog.example/zfs</link><guid>zfs-1</guid>
  <pubDate>Mon, 02 Nov 2026 10:00:00 GMT</pubDate>
  <description>&lt;p&gt;Pools&lt;/p&gt;&lt;p&gt;and &lt;b&gt;datasets&lt;/b&gt;&lt;/p&gt;&lt;script&gt;alert(1)&lt;/script&gt;</description>
  <category>FreeBSD</category></item>
<item><title>My cat</title><link>https://blog.example/cat</link><guid>cat-1</guid>
  <pubDate>Tue, 03 Nov 2026 10:00:00 GMT</pubDate><description>meow</description></item>
<item><title>Click me</title><link>javascript:alert(1)</link><guid>evil-1</guid></item>
<item><title>From the future</title><link>https://blog.example/future</link><guid>future-1</guid>
  <pubDate>Fri, 01 Jan 2038 00:00:00 GMT</pubDate><description>freebsd 20 is out</description></item>
<item><title>ZFS again</title><link>https://blog.example/zfs-dup</link><guid>zfs-1</guid></item>
</channel></rss>"#;

const ATOM: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
<title>Atom blog</title><id>https://atom.example/</id><updated>2026-11-01T00:00:00Z</updated>
<entry>
  <title type="html">Jails &lt;em&gt;explained&lt;/em&gt;</title>
  <id>https://atom.example/jails</id>
  <link rel="alternate" href="https://atom.example/jails"/>
  <updated>2026-11-01T08:00:00Z</updated>
  <author><name>Beastie</name></author>
  <summary>vnet and friends</summary>
</entry>
</feed>"#;

fn config(filter: &str) -> Config {
    Config::parse(&format!(
        "[site]\ntitle = \"P\"\nurl = \"https://p.example\"\nexcerpt_chars = 200\n\n[[feed]]\nname = \"A blog\"\nurl = \"https://blog.example/feed\"\nfilter = [{filter}]\n"
    ))
    .unwrap()
}

fn now() -> Timestamp {
    "2026-11-19T00:00:00Z".parse().unwrap()
}

async fn posts(document: &str, filter: &str, previous: &[Post]) -> Vec<Post> {
    let config = config(filter);
    parse(document.to_string(), &config.feeds[0], &config.site, previous, now()).await.unwrap()
}

fn ids(posts: &[Post]) -> Vec<&str> {
    posts.iter().map(|post| post.id.as_str()).collect()
}

fn by_id<'a>(posts: &'a [Post], id: &str) -> &'a Post {
    posts.iter().find(|post| post.id == id).unwrap()
}

#[tokio::test]
async fn rss_items_become_posts() {
    let posts = posts(RSS, "", &[]).await;
    assert_eq!(
        ids(&posts),
        ["zfs-1", "cat-1", "future-1"],
        "non-http link and duplicate guid are dropped"
    );

    let zfs = &posts[0];
    assert_eq!(zfs.title_html, "ZFS &amp; you");
    assert_eq!(zfs.link, "https://blog.example/zfs");
    assert_eq!(zfs.excerpt_html, "Pools and datasets");
    assert_eq!(zfs.published, "2026-11-02T10:00:00Z".parse::<Timestamp>().unwrap());
    assert_eq!(zfs.categories, ["FreeBSD"]);
}

#[tokio::test]
async fn filter_matches_title_category_or_excerpt() {
    assert_eq!(ids(&posts(RSS, "\"freebsd\"", &[]).await), ["zfs-1", "future-1"]);
    assert_eq!(ids(&posts(RSS, "\"CAT\"", &[]).await), ["cat-1"]);
}

#[tokio::test]
async fn future_date_falls_back_to_first_seen() {
    let first = posts(RSS, "", &[]).await;
    let future = by_id(&first, "future-1");
    assert_eq!(future.published, now());

    let mut remembered = future.clone();
    remembered.first_seen = "2026-11-10T00:00:00Z".parse().unwrap();
    remembered.published = remembered.first_seen;
    let second = posts(RSS, "", &[remembered.clone()]).await;
    let future = by_id(&second, "future-1");
    assert_eq!(
        future.published, remembered.first_seen,
        "a post does not jump to the top on every run"
    );
}

#[tokio::test]
async fn atom_entries_become_posts() {
    let posts = posts(ATOM, "", &[]).await;
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].title_html, "Jails explained");
    assert_eq!(posts[0].author.as_deref(), Some("Beastie"));
    assert_eq!(posts[0].excerpt_html, "vnet and friends");
    assert_eq!(posts[0].link, "https://atom.example/jails");
}

#[tokio::test]
async fn later_publisher_dates_revise_the_same_entry() {
    for (doc, old, new) in [
        (ATOM, "2026-11-01T08:00:00Z", "2026-11-02T08:00:00Z"),
        (RSS, "Mon, 02 Nov 2026", "Tue, 03 Nov 2026"),
    ] {
        let first = posts(doc, "", &[]).await;
        let next = posts(&doc.replace(old, new), "", &first).await;
        assert_eq!(ids(&first), ids(&next));
        assert_eq!(first[0].published, next[0].published);
        assert!(next[0].revised.is_some_and(|date| date > first[0].published));
    }
}

#[tokio::test]
async fn html_is_not_a_feed() {
    let config = config("");
    let result = parse(
        "<html><body>404</body></html>".to_string(),
        &config.feeds[0],
        &config.site,
        &[],
        now(),
    )
    .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn untitled_item_is_headed_by_its_own_text() {
    let document = RSS.replace("<title>My cat</title>", "").replace("<title>Click me</title>", "");
    let posts = posts(&document, "", &[]).await;
    let cat = by_id(&posts, "cat-1");
    assert_eq!((cat.title_html.as_str(), cat.excerpt_html.as_str()), ("meow", ""));
}
