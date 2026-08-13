mod api;
mod cli;

pub use api::Bitbucket;
pub use cli::BitbucketActions;

use anyhow::Result;
use serde_json::Value;

use crate::config::Config;
use crate::http::HttpClient;
use crate::module::AtlassianModule;

impl AtlassianModule for Bitbucket {
    type Action = BitbucketActions;

    fn module_name() -> &'static str {
        "bitbucket"
    }

    fn connect(cfg: &Config) -> Result<Self> {
        crate::config::check_ready(cfg, "bitbucket")?;
        Ok(Self::new(HttpClient::new(
            cfg.bitbucket_url.clone(),
            &cfg.bitbucket_token,
            cfg.allow_insecure_certs,
        )?))
    }

    async fn handle(&self, action: BitbucketActions) -> Result<Value> {
        match action {
            BitbucketActions::ListPrs(a) => self.list_prs(&a).await,
            BitbucketActions::CreatePr(a) => self.create_pr(&a).await,
            BitbucketActions::GetPr(a) => self.get_pr(&a).await,
            BitbucketActions::DiffPr(a) => self.get_pr_diff(&a).await,
            BitbucketActions::CommentsPr(a) => self.get_pr_comments(&a).await,
            BitbucketActions::CommentPr(a) => self.add_pr_comment(&a).await,
            BitbucketActions::ApprovePr(a) => self.approve_pr(&a).await,
            BitbucketActions::User { query, limit } => self.search_users(&query, limit).await,
        }
    }
}
