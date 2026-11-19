use super::*;

#[test]
fn escape_covers_markup_characters() {
    assert_eq!(
        escape(r#"<a href="x">Tom & 'Jerry'</a>"#),
        "&lt;a href=&quot;x&quot;&gt;Tom &amp; &#39;Jerry&#39;&lt;/a&gt;"
    );
}

#[test]
fn strip_removes_tags_and_scripts() {
    let html = r#"<p>Hello <b>world</b></p><script>alert(1)</script><img src="https://tracker.example/p.gif">"#;
    assert_eq!(strip(html), "Hello world");
}

#[test]
fn strip_keeps_paragraphs_apart_but_not_inline_tags() {
    assert_eq!(strip("<p>one</p><p>two</p>"), "one two");
    assert_eq!(strip("line<br>break"), "line break");
    assert_eq!(strip("<b>F</b>reeBSD"), "FreeBSD");
}

#[test]
fn strip_output_stays_escaped() {
    assert_eq!(strip("a &lt;script&gt; &amp; b"), "a &lt;script&gt; &amp; b");
}

#[test]
fn truncate_leaves_short_text_alone() {
    assert_eq!(truncate("short text", 50), "short text");
}

#[test]
fn truncate_cuts_on_a_word_boundary() {
    assert_eq!(truncate("the quick brown fox jumps", 12), "the quick…");
}

#[test]
fn truncate_never_splits_an_entity() {
    assert_eq!(truncate("aaaa&amp;bbbbbbbb", 6), "aaaa…");
    assert_eq!(truncate("fish &amp; chips and more", 10), "fish &amp;…");
}
