mod api;
mod cli;

pub use api::Jira;
pub use cli::JiraActions;

use anyhow::Result;
use serde_json::Value;

use crate::config::Config;
use crate::http::HttpClient;
use crate::module::AtlassianModule;

impl AtlassianModule for Jira {
    type Action = JiraActions;

    fn module_name() -> &'static str {
        "jira"
    }

    fn connect(cfg: &Config) -> Result<Self> {
        crate::config::check_ready(cfg, "jira")?;
        Ok(Self::new(HttpClient::new(
            cfg.jira_url.clone(),
            &cfg.jira_token,
            cfg.allow_insecure_certs,
        )?))
    }

    async fn handle(&self, action: JiraActions) -> Result<Value> {
        match action {
            JiraActions::Get { key, comments_limit } => self.get_issue(&key, comments_limit).await,
            JiraActions::Comment { key, text } => self.add_comment(&key, &text).await,
            JiraActions::Transition { key, status } => self.transition(&key, &status).await,
            JiraActions::Search { jql, limit } => self.search_issues(&jql, limit).await,
            JiraActions::Create(a) => self.create_issue(&a).await,
            JiraActions::Update(a) => self.update_issue(&a).await,
            JiraActions::Assign { key, assignee } => self.assign_issue(&key, &assignee).await,
            JiraActions::User { query, limit } => self.search_users(&query, limit).await,
            JiraActions::AssignableUsers { key, query, limit } => {
                self.search_assignable_users(&key, query.as_deref(), limit).await
            }
        }
    }
}
