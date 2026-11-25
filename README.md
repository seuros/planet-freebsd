# Planet BSD

Planet BSD collects news and writing from across the BSDs. One binary polls the feeds listed
in `planet.toml` and writes a site any web server can host. No database, no
daemon, no JavaScript.

[bsdfeed.seuros.net](https://bsdfeed.seuros.net/) is the temporary home of this
independent proof of concept. After the proof of concept, the plan is to transfer
the repository and the production feed to the FreeBSD Foundation.

```sh
cargo build --release
./target/release/planet check    # validate planet.toml, no network
./target/release/planet build    # poll every feed, write ./public
```

`build` writes `index.html`, `atom.xml` (the combined feed), `opml.xml` (the
subscription list) and `status.html` (per-feed health) into `--out`, and keeps
what it needs between runs in `--state` (`state.json`).

## Adding a feed

Append a block to `planet.toml` and send it for review. The site lists sources
tagged `org` first, then all other sources, alphabetically by name within each
group. No manual sorting is needed.

```toml
[[feed]]
name = "Your Name"
tags = ["freebsd", "personal"]    # optional, shown as colored labels; does not filter posts
url = "https://example.org/tags/freebsd/feed.xml"
site = "https://example.org"      # optional, where the name links to
filter = ["freebsd", "zfs"]        # optional, for blogs that are not only about FreeBSD
```

Prefer a tag or category feed. Where the blog has none, `filter` keeps only posts
mentioning one of the words in their title, categories or excerpt. RSS 2.0 and
Atom 1.0 are read; RSS 1.0 (RDF) is not.

For a mixed-topic blog, set `require_categories = ["bsd", "freebsd", "openbsd"]`
on its feed to publish only posts carrying at least one of those RSS or Atom
categories. Matching ignores ASCII case but requires the whole tag. A mention
in the title or excerpt doesn't count. Changes apply on an offline rebuild.

## What it does with a feed

- **Conditional GET.** `ETag` / `Last-Modified` are remembered, so an unchanged
  feed costs a 304. Transient failures (network, 429, 5xx) are retried with
  backoff. Bodies are capped at 8 MB.
- **Excerpts only.** Publisher markup never reaches the page: tags are stripped,
  the text is cut to `excerpt_chars`, and the title links to the original. Links
  that are not `http(s)` drop the post. The pages carry a CSP that allows no
  scripts and no remote resources.
- **Dates.** A post with no date, or one in the future, is dated when it was
  first seen, so a broken clock cannot pin a post to the top.
- **Fairness.** At most `max_posts_per_feed` posts per feed, nothing older than
  `max_age_days`, `max_posts` overall.
- **Health.** A failing feed keeps its cached posts and is reported on stderr and
  on `status.html`. After `mute_after_failures` consecutive failed runs its
  posts are hidden until it answers again.

`build` exits non-zero only when every feed failed. After changing a `filter` or
`excerpt_chars`, run `build --force` once: an unchanged feed otherwise answers
304 and keeps its already-processed posts. `build --offline` re-renders from the
cache, which is what you want while editing `templates/`.

## Running it on FreeBSD

Build and install (needs Rust 1.99.1 or newer; if the `rust` package is older,
install `rustup-init` instead and use its toolchain):

```sh
pkg install rust git
git clone https://github.com/seuros/planet-freebsd
cd planet-freebsd
cargo build --release
install -m 755 target/release/planet /usr/local/bin/planet
install -m 644 planet.toml /usr/local/etc/planet.toml
```

Give it an unprivileged user and somewhere to write:

```sh
pw useradd planet -d /nonexistent -s /usr/sbin/nologin -c "Planet aggregator"
install -d -o planet -g planet /var/db/planet /usr/local/www/planet
```

Poll every 30 minutes from `/usr/local/etc/cron.d/planet`:

```cron
*/30 * * * * planet /usr/local/bin/planet -c /usr/local/etc/planet.toml build --state /var/db/planet/state.json --out /usr/local/www/planet >/dev/null
```

Failing feeds are printed on stderr, so cron mails them. Point any web server at
`/usr/local/www/planet`; for nginx:

```nginx
server {
    listen 80;
    server_name planet.example.org;
    root /usr/local/www/planet;
    index index.html;
}
```

Files are replaced by rename, so the web server never serves half a page.

## Development

```sh
cargo test                   # unit tests plus the binary against a local HTTP server
cargo clippy --all-targets
```

Templates live in `templates/` and are compiled into the binary.
