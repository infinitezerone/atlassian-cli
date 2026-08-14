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
            ConfluenceActions::Search {
                query,
                limit,
                title_only,
                space,
            } => self.search(&query, limit, title_only, space.as_deref()).await,
            ConfluenceActions::Get {
                id,
                title_only,
                raw,
                max_chars,
                offset,
            } => self.get_page(&id, title_only, raw, max_chars, offset).await,
            ConfluenceActions::Children { id, limit } => self.get_children(&id, limit).await,
            ConfluenceActions::Spaces { query, limit } => self.list_spaces(query.as_deref(), limit).await,
            ConfluenceActions::Attachments { id, limit } => self.list_attachments(&id, limit).await,
            ConfluenceActions::Attach { id, file, comment } => {
                self.attach_file(&id, &file, comment.as_deref()).await
            }
            ConfluenceActions::Create(a) => self.create_page(&a).await,
            ConfluenceActions::Update(a) => self.update_page(&a).await,
        }
    }
}
