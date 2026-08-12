mod api;
mod cli;

pub use api::Confluence;
pub use cli::ConfluenceActions;

use anyhow::Result;
use serde_json::Value;

use crate::config::Config;
use crate::http::HttpClient;
use crate::module::AtlassianModule;

impl AtlassianModule for Confluence {
    type Action = ConfluenceActions;

    fn module_name() -> &'static str {
        "confluence"
    }

    fn connect(cfg: &Config) -> Result<Self> {
        crate::config::check_ready(cfg, "confluence")?;
        Ok(Self::new(HttpClient::new(
            cfg.confluence_url.clone(),
            &cfg.confluence_token,
            cfg.allow_insecure_certs,
        )?))
    }

    async fn handle(&self, action: ConfluenceActions) -> Result<Value> {
        match action {
            ConfluenceActions::Search { query, limit } => self.search(&query, limit).await,
            ConfluenceActions::Get {
                id,
                raw,
                max_chars,
                offset,
            } => self.get_page(&id, raw, max_chars, offset).await,
        }
    }
}
