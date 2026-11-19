use std::path::PathBuf;

use anyhow::bail;
use jiff::Timestamp;
use usage_rs::{Args, RunAsyncWith};

use crate::app::App;
use crate::config::Config;
use crate::refresh::refresh;
use crate::render::render;
use crate::state::State;

/// Poll every feed and write the site
///
/// Meant for cron. A feed that fails keeps its cached posts and is reported on
/// stderr; the run only fails when no feed at all could be reached.
#[derive(Args, Debug)]
#[usage(effect = "write")]
pub struct Build {
    /// Directory the site is written to
    #[usage(short = 'o', long, value_name = "DIR", default = "public")]
    pub out: PathBuf,
    /// Cache of feed validators, health and posts
    #[usage(long, value_name = "FILE", default = "state.json")]
    pub state: PathBuf,
    /// Render from the cache without touching the network
    #[usage(long)]
    pub offline: bool,
    /// Download every feed again, ignoring cached validators
    ///
    /// Needed after changing a feed's `filter` or `excerpt_chars`: an
    /// unchanged feed otherwise answers 304 and keeps its cached posts.
    #[usage(long)]
    pub force: bool,
}

impl RunAsyncWith<&App> for Build {
    type Output = anyhow::Result<()>;

    async fn run_async_with(self, app: &App) -> Self::Output {
        let config = Config::load(&app.config)?;
        let mut state = State::load(&self.state)?;
        let now = Timestamp::now();

        if !self.offline {
            refresh(&config, &mut state, now, self.force).await;
            state.save(&self.state)?;
        }
        let posts = render(&config, &state, &self.out, now).await?;

        let mut failing = 0;
        for source in &config.feeds {
            let Some(feed) = state.feed(source).filter(|feed| feed.failures > 0) else {
                continue;
            };
            failing += 1;
            let error = feed.last_error.as_deref().unwrap_or("unknown error");
            eprintln!("planet: {}: {error} ({} in a row)", source.name, feed.failures);
        }
        let total = config.feeds.len();
        println!("{}/{total} feeds ok, {posts} posts -> {}", total - failing, self.out.display());

        if !self.offline && failing == total {
            bail!("every feed failed; is the network up?");
        }
        Ok(())
    }
}
