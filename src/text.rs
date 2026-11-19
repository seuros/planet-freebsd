//! Turning feed markup into text that is safe to embed in a page.
//!
//! Everything here returns *HTML-safe text*: no tags, entities escaped. The
//! page shows excerpts, never the publisher's markup, so there is nothing to
//! allow-list and no images to hotlink.

/// Tags that end a line of text. Stripping them without leaving a space would
/// glue the last word of one paragraph to the first word of the next.
const BLOCK_TAGS: &[&str] =
    &["<p", "</p", "<br", "<div", "</div", "<li", "<h", "<tr", "<td", "<pre", "<blockquote"];

/// Escape plain text for HTML.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// Drop every tag (and the contents of `<script>`/`<style>`), collapse
/// whitespace.
pub fn strip(html: &str) -> String {
    let mut spaced = String::with_capacity(html.len() + 16);
    let mut rest = html;
    while let Some(at) = rest.find('<') {
        spaced.push_str(&rest[..at]);
        let tail = &rest[at..];
        if BLOCK_TAGS.iter().any(|tag| starts_with_ignore_case(tail, tag)) {
            spaced.push(' ');
        }
        spaced.push('<');
        rest = &tail[1..];
    }
    spaced.push_str(rest);

    let clean = ammonia::Builder::empty().clean(&spaced).to_string();
    clean.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn starts_with_ignore_case(text: &str, prefix: &str) -> bool {
    text.as_bytes()
        .get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix.as_bytes()))
}

/// Shorten HTML-safe text to about `max_chars`, on a word boundary, without
/// cutting an entity in half.
pub fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for word in text.split(' ') {
        let len = word.chars().count();
        if used + len > max_chars {
            break;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
        used += len + 1;
    }
    if out.is_empty() {
        // One unbroken run longer than the limit (text without spaces).
        out = text.chars().take(max_chars).collect();
        if let Some(amp) = out.rfind('&')
            && !out[amp..].contains(';')
        {
            out.truncate(amp);
        }
    }
    out.push('…');
    out
}

#[cfg(test)]
mod tests;
