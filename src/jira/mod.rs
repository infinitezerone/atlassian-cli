mod attachment;
mod cli;
mod comment;
mod issue;
mod metadata;
mod transition;
mod worklog;

pub use cli::*;

use serde_json::Value;

use crate::config::Config;
use crate::error::AppError;
use crate::http::HttpClient;
use crate::module::{AtlassianModule, WritePolicy};

/// Jira 产品客户端:聚合各子领域 API 方法
pub struct Jira {
    http: HttpClient,
    policy: WritePolicy,
}

impl Jira {
    pub fn new(http: HttpClient, policy: WritePolicy) -> Self {
        Self { http, policy }
    }
}

impl AtlassianModule for Jira {
    type Action = JiraActions;

    fn module_name() -> &'static str {
        "jira"
    }

    fn connect(cfg: &Config, policy: WritePolicy) -> Result<Self, AppError> {
        crate::config::check_ready(cfg, "jira")?;
        Ok(Self::new(
            HttpClient::new(
                cfg.jira_url.clone(),
                &cfg.jira_token,
                cfg.allow_insecure_certs,
            )?,
            policy,
        ))
    }

    async fn handle(&self, action: JiraActions) -> Result<Value, AppError> {
        match action {
            JiraActions::Get(a) => self.get_issue(&a).await,
            JiraActions::Comment(a) => self.add_comment(&a.key, a.get_text()?).await,
            JiraActions::CommentUpdate(a) => {
                self.update_comment(&a.key, &a.comment_id, a.get_text()?).await
            }
            JiraActions::CommentDelete { key, comment_id } => {
                self.delete_comment(&key, &comment_id).await
            }
            JiraActions::Transition { key, status } => self.transition(&key, &status).await,
            JiraActions::Search {
                jql,
                limit,
                fields,
                start_at,
            } => {
                self.search_issues(&jql, limit, fields.as_deref(), start_at)
                    .await
            }
            JiraActions::Create(a) => self.create_issue(&a).await,
            JiraActions::Update(a) => self.update_issue(&a).await,
            JiraActions::Assign(a) => self.assign_issue(&a.key, a.get_assignee()?).await,
            JiraActions::User { query, limit } => self.search_users(&query, limit).await,
            JiraActions::AssignableUsers { key, query, limit } => {
                self.search_assignable_users(&key, query.as_deref(), limit)
                    .await
            }
            JiraActions::SuggestFields => self.suggest_fields().await,
            JiraActions::SuggestValues {
                field,
                query,
                limit,
            } => {
                self.suggest_values(&field, query.as_deref(), limit)
                    .await
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
            } => {
                self.link_issue(&from_key, &to_key, &r#type, comment.as_deref())
                    .await
            }
            JiraActions::Attachments { key } => self.list_attachments(&key).await,
            JiraActions::Attach { key, file } => self.attach_file(&key, &file).await,
            JiraActions::AttachmentDownload {
                key,
                attachment,
                output,
            } => {
                self.download_attachment(&key, &attachment, output.as_deref())
                    .await
            }
            JiraActions::Fields {
                query,
                custom_only,
                limit,
            } => self.list_fields(query.as_deref(), custom_only, limit).await,
            JiraActions::Projects { query } => self.list_projects(query.as_deref()).await,
            JiraActions::IssueTypes { project, limit } => {
                self.get_issue_types(project.as_deref(), limit).await
            }
            JiraActions::Watchers { key, add, remove } => {
                self.manage_watchers(&key, add.as_deref(), remove.as_deref())
                    .await
            }
            JiraActions::AttachmentDelete { key, attachment } => {
                self.delete_attachment(&key, &attachment).await
            }
            JiraActions::BulkCreate(a) => self.bulk_create_issues(&a).await,
            JiraActions::Clone(a) => self.clone_issue(&a).await,
        }
    }
}
