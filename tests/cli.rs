//! The built binary against a local HTTP server: no real network.

use std::fmt::Write as _;
use std::io::{BufRead, BufReader, Write as _};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Output};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

const FEED: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0"><channel>
<title>Beastie's blog</title><link>https://beastie.example/</link><description>daemon notes</description>
<item><title>Hello &lt;Beastie&gt;</title><link>https://beastie.example/hello</link>
  <guid>https://beastie.example/hello</guid><pubDate>Mon, 02 Nov 2026 10:00:00 GMT</pubDate>
  <description>&lt;p&gt;First &lt;img src="https://tracker.example/p.gif"&gt;post&lt;/p&gt;</description></item>
</channel></rss>"#;

/// How the fake server answered, by kind.
#[derive(Default)]
struct Hits {
    full: AtomicUsize,
    not_modified: AtomicUsize,
}

/// Serves `FEED` with an `ETag` at `/feed.xml`, 304 when the client already
/// has it, 500 at any other path.
fn serve() -> (String, Arc<Hits>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let hits = Arc::new(Hits::default());
    let counted = Arc::clone(&hits);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let mut head = Vec::new();
            for line in BufReader::new(stream.try_clone().unwrap()).lines() {
                let line = line.unwrap();
                if line.is_empty() {
                    break;
                }
                head.push(line.to_lowercase());
            }
            let response = if !head[0].starts_with("get /feed.xml ") {
                "HTTP/1.1 500 Internal Server Error\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".to_string()
            } else if head.iter().any(|line| line == "if-none-match: \"v1\"") {
                counted.not_modified.fetch_add(1, Ordering::SeqCst);
                "HTTP/1.1 304 Not Modified\r\netag: \"v1\"\r\nconnection: close\r\n\r\n".to_string()
            } else {
                counted.full.fetch_add(1, Ordering::SeqCst);
                format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/rss+xml\r\netag: \"v1\"\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{FEED}",
                    FEED.len()
                )
            };
            stream.write_all(response.as_bytes()).unwrap();
        }
    });
    (base, hits)
}

fn write_config(dir: &Path, feeds: &[(&str, String)]) {
    let mut config =
        "[site]\ntitle = \"Test Planet\"\nurl = \"https://planet.example\"\n".to_string();
    for (name, url) in feeds {
        write!(config, "\n[[feed]]\nname = \"{name}\"\nurl = \"{url}\"\n").unwrap();
    }
    std::fs::write(dir.join("planet.toml"), config).unwrap();
}

fn planet(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_planet")).current_dir(dir).args(args).output().unwrap()
}

fn build(dir: &Path) -> Output {
    planet(dir, &["build"])
}

fn page(dir: &Path, name: &str) -> String {
    std::fs::read_to_string(dir.join("public").join(name)).unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn build_writes_the_site_and_revalidates_on_the_next_run() {
    let (base, hits) = serve();
    let dir = tempfile::tempdir().unwrap();
    write_config(dir.path(), &[("Beastie", format!("{base}/feed.xml"))]);

    let first = build(dir.path());
    assert!(first.status.success(), "{}", text(&first.stderr));
    assert_eq!(text(&first.stdout), "1/1 feeds ok, 1 posts -> public\n");

    let index = page(dir.path(), "index.html");
    assert!(index.contains("Hello &lt;Beastie&gt;"), "{index}");
    assert!(index.contains(r#"href="https://beastie.example/hello""#));
    assert!(index.contains("First post"));
    assert!(!index.contains("tracker.example"), "publisher markup never reaches the page");
    for file in ["atom.xml", "opml.xml", "status.html"] {
        assert!(dir.path().join("public").join(file).is_file(), "{file}");
    }

    let second = build(dir.path());
    assert!(second.status.success(), "{}", text(&second.stderr));
    assert_eq!(
        text(&second.stdout),
        "1/1 feeds ok, 1 posts -> public\n",
        "a 304 keeps the cached post"
    );
    assert_eq!(
        (hits.full.load(Ordering::SeqCst), hits.not_modified.load(Ordering::SeqCst)),
        (1, 1)
    );

    let forced = planet(dir.path(), &["build", "--force"]);
    assert!(forced.status.success(), "{}", text(&forced.stderr));
    assert_eq!(hits.full.load(Ordering::SeqCst), 2, "--force skips the conditional GET");
}

#[test]
fn a_failing_feed_is_reported_but_does_not_fail_the_run() {
    let (base, _hits) = serve();
    let dir = tempfile::tempdir().unwrap();
    write_config(
        dir.path(),
        &[("Beastie", format!("{base}/feed.xml")), ("Broken", format!("{base}/nope.xml"))],
    );

    let out = build(dir.path());
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "1/2 feeds ok, 1 posts -> public\n");
    assert_eq!(text(&out.stderr), "planet: Broken: HTTP 500 (1 in a row)\n");

    let status = page(dir.path(), "status.html");
    assert!(status.contains("failing") && status.contains("HTTP 500"), "{status}");
}

#[test]
fn build_fails_when_every_feed_fails() {
    let (base, _hits) = serve();
    let dir = tempfile::tempdir().unwrap();
    write_config(dir.path(), &[("Broken", format!("{base}/nope.xml"))]);

    let out = build(dir.path());
    assert!(!out.status.success());
    assert!(text(&out.stderr).contains("every feed failed"), "{}", text(&out.stderr));
    assert!(dir.path().join("public/index.html").is_file(), "the site is still written");
}

#[test]
fn check_validates_without_a_network() {
    let dir = tempfile::tempdir().unwrap();
    write_config(dir.path(), &[("One", "https://one.example/feed".to_string())]);
    let ok = planet(dir.path(), &["check"]);
    assert!(ok.status.success());
    assert_eq!(text(&ok.stdout), "planet.toml: ok, 1 feeds\n");

    write_config(dir.path(), &[("One", "ftp://one.example/feed".to_string())]);
    let bad = planet(dir.path(), &["check"]);
    assert!(!bad.status.success());
    assert!(text(&bad.stderr).contains("not an http(s) URL"), "{}", text(&bad.stderr));
}
