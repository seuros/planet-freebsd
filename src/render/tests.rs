use super::*;

fn config() -> Config {
    Config::parse(
        r#"
[site]
title = "Planet <Test>"
url = "https://planet.example"
max_posts = 3
max_posts_per_feed = 2
max_age_days = 30
mute_after_failures = 2

[[feed]]
name = "Busy"
url = "https://busy.example/feed"

[[feed]]
name = "Quiet & Co"
tags = ["dragonflybsd", "personal"]
url = "https://quiet.example/feed"
site = "https://quiet.example"
"#,
    )
    .unwrap()
}

fn at(text: &str) -> Timestamp {
    text.parse().unwrap()
}

fn post(id: &str, published: &str) -> Post {
    Post {
        id: format!("https://posts.example/{id}"),
        title_html: format!("Title {id} &amp; more"),
        link: format!("https://posts.example/{id}"),
        author: None,
        revised: None,
        published: at(published),
        publication_confirmed: true,
        first_seen: at(published),
        excerpt_html: "An excerpt".to_string(),
        categories: Vec::new(),
    }
}

fn state() -> State {
    let mut state = State::default();
    let busy = FeedState {
        posts: vec![
            post("b1", "2026-11-18T10:00:00Z"),
            post("b2", "2026-11-17T10:00:00Z"),
            post("b3", "2026-11-16T10:00:00Z"),
            post("ancient", "2020-01-01T00:00:00Z"),
        ],
        ..FeedState::default()
    };
    let quiet =
        FeedState { posts: vec![post("q1", "2026-11-17T12:00:00Z")], ..FeedState::default() };
    state.feeds.insert("https://busy.example/feed".to_string(), busy);
    state.feeds.insert("https://quiet.example/feed".to_string(), quiet);
    state
}

fn feed_mut<'a>(state: &'a mut State, host: &str) -> &'a mut FeedState {
    state.feeds.get_mut(&format!("https://{host}.example/feed")).unwrap()
}

fn now() -> Timestamp {
    at("2026-11-19T00:00:00Z")
}

fn selected(config: &Config, state: &State) -> Vec<String> {
    select(config, state, now(), None)
        .iter()
        .map(|entry| entry.post.link.rsplit('/').next().unwrap().to_string())
        .collect()
}

#[test]
fn select_caps_per_feed_drops_old_posts_and_sorts_newest_first() {
    assert_eq!(selected(&config(), &state()), ["b1", "q1", "b2"]);
}

#[test]
fn required_categories_match_post_tags_before_the_feed_cap() {
    let (mut config, mut state) = (config(), state());
    config.feeds[0].require_categories = vec!["bsd".into()];
    let posts = &mut feed_mut(&mut state, "busy").posts;
    posts[0].title_html = "BSD".into();
    posts[1].categories = vec!["not-bsd".into()];
    posts[2].categories = vec!["BSD".into()];
    assert_eq!(selected(&config, &state), ["q1", "b3"]);
    assert_eq!(rows(&config.site, &[&config.feeds[0]], &state)[0].posts, 1);
}

#[test]
fn select_skips_muted_feeds_but_keeps_failing_ones() {
    let (config, mut state) = (config(), state());
    feed_mut(&mut state, "busy").failures = 1;
    assert_eq!(selected(&config, &state), ["b1", "q1", "b2"]);
    feed_mut(&mut state, "busy").failures = 2;
    assert_eq!(selected(&config, &state), ["q1"]);
}

#[tokio::test]
async fn render_writes_every_file_escaped() {
    let (config, mut state) = (config(), state());
    feed_mut(&mut state, "quiet").failures = 1;
    feed_mut(&mut state, "quiet").posts[0].author = Some("undefined <blog@example.org>".into());
    feed_mut(&mut state, "quiet").last_error = Some("HTTP 503".to_string());
    let out = tempfile::tempdir().unwrap();

    assert_eq!(render(&config, &state, out.path(), now()).await.unwrap(), 3);
    let read = |name: &str| std::fs::read_to_string(out.path().join(name)).unwrap();

    let index = read("index.html");
    assert!(
        index.contains("Planet &#60;Test&#62;") || index.contains("Planet &lt;Test&gt;"),
        "site title is escaped"
    );
    assert!(
        index.contains("Title b1 &amp; more"),
        "post titles are embedded as the safe HTML they already are"
    );
    assert!(index.contains(r#"href="https://posts.example/q1""#));
    assert!(!index.contains("ancient"));
    assert_eq!(index.matches("data-tag=\"personal\">+personal</span>").count(), 2);
    let scoped = read("tags/dragonflybsd/atom.xml");
    assert!(scoped.contains("/tags/dragonflybsd/atom.xml") && scoped.contains("posts.example/q1"));
    assert!(!scoped.contains("posts.example/b1"));

    let status = read("status.html");
    assert!(status.contains("failing") && status.contains("HTTP 503"));

    let opml = read("opml.xml");
    assert!(
        opml.contains(r#"xmlUrl="https://quiet.example/feed""#)
            && opml.contains(r#"htmlUrl="https://quiet.example""#)
    );

    let atom = read("atom.xml");
    let subscribe = read("subscribe.html");
    assert!(subscribe.contains("https://planet.example/tags/openbsd/atom.xml"));
    assert!(read("tags/openbsd/index.html").contains("href=\"../../subscribe.html\""));
    assert!(!index.contains("undefined") && !atom.contains("undefined"));
    assert!(atom.contains("<id>https://posts.example/b1</id>"), "{atom}");
    assert!(
        atom.contains(r#"<title type="html"><![CDATA[Title b1 &amp; more]]></title>"#),
        "{atom}"
    );
}
