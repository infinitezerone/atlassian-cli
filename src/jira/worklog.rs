use serde_json::{json, Value};

use super::cli::{AddWorklogArgs, DeleteWorklogArgs, ListWorklogsArgs};
use super::Jira;
use crate::error::AppError;
use crate::utils::parse_jira_key;

impl Jira {
    /// POST /rest/api/2/issue/{key}/worklog (在单子上登记工作工时与日志)
    pub async fn add_worklog(&self, a: &AddWorklogArgs) -> Result<Value, AppError> {
        let key = parse_jira_key(&a.key_or_url);
        let path = format!("/rest/api/2/issue/{}/worklog", urlencoding::encode(&key));

        let time_spent = a.get_time_spent()?;
        // 字段值校验:拦 AI 常见的工时格式错 / 开始时间错 / 提及语法错
        crate::utils::validate_time_spent(time_spent)?;
        if let Some(ref c) = a.comment {
            if !c.trim().is_empty() {
                crate::utils::validate_mentions(c)?;
            }
        }
        if let Some(ref s) = a.started {
            if !s.trim().is_empty() {
                crate::utils::validate_started(s)?;
            }
        }

        let mut body = json!({
            "timeSpent": time_spent.trim(),
        });

        if let Some(ref c) = a.comment {
            if !c.trim().is_empty() {
                body["comment"] = json!(c.trim());
            }
        }

        if let Some(ref s) = a.started {
            if !s.trim().is_empty() {
                body["started"] = json!(format_jira_started_time(s));
            }
        }

        if self.policy.dry_run {
            return Ok(crate::module::preview_json("jira.worklog-add", "POST", &path, &key, Some(&body), None));
        }
        crate::module::require_confirmed(&self.policy)?;
        let raw = self.http.post(&path, body).await?;
        if crate::module::is_replayed(&raw) {
            return Ok(raw);
        }

        Ok(json!({
            "status": "success",
            "issue_key": key,
            "worklog_id": raw["id"].as_str().unwrap_or(""),
            "time_spent": raw["timeSpent"].as_str().unwrap_or(time_spent),
            "started": raw["started"].as_str().unwrap_or(""),
            "author": raw["author"]["displayName"].as_str().or(raw["author"]["name"].as_str()).unwrap_or(""),
            "comment": raw["comment"].as_str().unwrap_or(""),
            "url": format!("{}/browse/{}", self.http.base_url(), key),
        }))
    }

    /// GET /rest/api/2/issue/{key}/worklog (查询单子上的历史工时日志记录)
    pub async fn list_worklogs(&self, a: &ListWorklogsArgs) -> Result<Value, AppError> {
        let key = parse_jira_key(&a.key_or_url);
        let path = format!("/rest/api/2/issue/{}/worklog", urlencoding::encode(&key));

        let raw = self.http.get(&path).await?;
        let worklogs = raw["worklogs"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .map(|w| {
                        json!({
                            "id": w["id"].as_str().unwrap_or(""),
                            "author": w["author"]["displayName"].as_str().or(w["author"]["name"].as_str()).unwrap_or(""),
                            "author_username": w["author"]["name"].as_str().unwrap_or(""),
                            "time_spent": w["timeSpent"].as_str().unwrap_or(""),
                            "time_spent_seconds": w["timeSpentSeconds"].as_u64().unwrap_or(0),
                            "started": w["started"].as_str().unwrap_or(""),
                            "created": w["created"].as_str().unwrap_or(""),
                            "comment": w["comment"].as_str().unwrap_or(""),
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Ok(json!({
            "issue_key": key,
            "total_count": worklogs.len(),
            "worklogs": worklogs,
        }))
    }

    /// DELETE /rest/api/2/issue/{key}/worklog/{id} (删除指定工时记录)
    pub async fn delete_worklog(&self, a: &DeleteWorklogArgs) -> Result<Value, AppError> {
        let key = parse_jira_key(&a.key_or_url);
        let path = format!(
            "/rest/api/2/issue/{}/worklog/{}",
            urlencoding::encode(&key),
            urlencoding::encode(a.worklog_id.trim())
        );

        if self.policy.dry_run {
            return Ok(crate::module::preview_json(
                "jira.worklog-delete",
                "DELETE",
                &path,
                a.worklog_id.trim(),
                None,
                None,
            ));
        }
        crate::module::require_confirmed(&self.policy)?;
        let raw = self.http.delete(&path).await?;
        if crate::module::is_replayed(&raw) {
            return Ok(raw);
        }

        Ok(json!({
            "status": "success",
            "issue_key": key,
            "worklog_id": a.worklog_id.trim(),
            "deleted": true,
            "url": format!("{}/browse/{}", self.http.base_url(), key),
        }))
    }
}

pub(crate) fn format_jira_started_time(s: &str) -> String {
    let trimmed = s.trim();
    if trimmed.contains('T') || trimmed.contains('+') {
        return trimmed.to_string();
    }
    if trimmed.len() == 10 {
        return format!("{}T09:00:00.000+0800", trimmed);
    }
    trimmed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_jira_started_time() {
        assert_eq!(format_jira_started_time("2026-08-13"), "2026-08-13T09:00:00.000+0800");
        assert_eq!(format_jira_started_time("2026-08-13T10:00:00.000+0800"), "2026-08-13T10:00:00.000+0800");
    }
}
