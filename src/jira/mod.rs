mod api;
mod cli;

pub use api::Jira;
pub use cli::JiraActions;

use serde_json::Value;

use crate::error::AppError;

use crate::config::Config;
use crate::http::HttpClient;
use crate::module::{AtlassianModule, WritePolicy};

impl AtlassianModule for Jira {
    type Action = JiraActions;

    fn module_name() -> &'static str {
        "jira"
    }

    fn connect(cfg: &Config, policy: WritePolicy) -> Result<Self, AppError> {
        crate::config::check_ready(cfg, "jira")?;
        Ok(Self::new(HttpClient::new(
            cfg.jira_url.clone(),
            &cfg.jira_token,
            cfg.allow_insecure_certs,
        )?,
            policy))
    }

    async fn handle(&self, action: JiraActions) -> Result<Value, AppError> {
        match action {
            JiraActions::Get(a) => self.get_issue(&a).await,
            JiraActions::Comment { key, text } => self.add_comment(&key, &text).await,
            JiraActions::Transition { key, status } => self.transition(&key, &status).await,
            JiraActions::Search { jql, limit, fields, start_at } => {
                self.search_issues(&jql, limit, fields.as_deref(), start_at).await
            }
            JiraActions::Create(a) => self.create_issue(&a).await,
            JiraActions::Update(a) => self.update_issue(&a).await,
            JiraActions::Assign { key, assignee } => self.assign_issue(&key, &assignee).await,
            JiraActions::User { query, limit } => self.search_users(&query, limit).await,
            JiraActions::AssignableUsers { key, query, limit } => {
                self.search_assignable_users(&key, query.as_deref(), limit).await
            }
            JiraActions::WorklogAdd(a) => self.add_worklog(&a).await,
            JiraActions::WorklogList(a) => self.list_worklogs(&a).await,
            JiraActions::WorklogDelete(a) => self.delete_worklog(&a).await,
            JiraActions::Transitions { key } => self.get_transitions(&key).await,
            JiraActions::Link {
                from_key,
                to_key,
                r#type,
                comment,
            } => self.link_issue(&from_key, &to_key, &r#type, comment.as_deref()).await,
            JiraActions::Attachments { key } => self.list_attachments(&key).await,
            JiraActions::Attach { key, file } => self.attach_file(&key, &file).await,
        }
    }
}
