use super::*;

const MINIMAL: &str = r#"
[site]
title = "Planet"
url = "https://planet.example"

[[feed]]
name = "One"
url = "https://one.example/feed"
"#;

#[test]
fn minimal_config_gets_defaults() {
    let config = Config::parse(MINIMAL).unwrap();
    assert_eq!(config.problems(), Vec::<String>::new());
    assert_eq!(config.site.max_posts, 60);
    assert_eq!(config.site.mute_after_failures, 5);
    assert_eq!(config.feeds[0].filter, Vec::<String>::new());
}

#[test]
fn unknown_keys_are_rejected() {
    let text = MINIMAL.replace("name = \"One\"", "name = \"One\"\nurll = \"typo\"");
    assert!(Config::parse(&text).is_err());
}

#[test]
fn duplicate_and_non_http_feeds_are_reported() {
    let text = format!(
        "{MINIMAL}\n[[feed]]\nname = \"Dup\"\nurl = \"https://one.example/feed\"\n\n[[feed]]\nname = \"Local\"\nurl = \"file:///etc/passwd\"\n"
    );
    let problems = Config::parse(&text).unwrap().problems();
    assert_eq!(problems.len(), 2, "{problems:?}");
    assert!(problems[0].contains("listed twice"));
    assert!(problems[1].contains("not an http(s) URL"));
}

#[test]
fn empty_feed_list_is_a_problem() {
    let config = Config::parse("[site]\ntitle = \"P\"\nurl = \"https://p.example\"\n").unwrap();
    assert_eq!(config.problems(), ["no [[feed]] entries"]);
}
