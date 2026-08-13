use anyhow::{bail, Result};
use serde_json::{json, Value};

use super::cli::{AddWorklogArgs, CreateIssueArgs, DeleteWorklogArgs, ListWorklogsArgs, UpdateIssueArgs};
use crate::http::HttpClient;
use crate::utils::{parse_jira_key, parse_username};

/// Jira 产品客户端:一个方法 = 一个 API,新增 API 就在这加方法
pub struct Jira {
    http: HttpClient,
}

impl Jira {
    pub fn new(http: HttpClient) -> Self {
        Self { http }
    }

    /// GET /rest/api/2/issue/{key} -> 裁剪字段 (支持直接传入 Issue Key 或完整网页 URL)
    pub async fn get_issue(&self, key_or_url: &str, comments_limit: u32) -> Result<Value> {
        let key = parse_jira_key(key_or_url);
        let enc_key = urlencoding::encode(&key);
        let raw = self
            .http
            .get(&format!("/rest/api/2/issue/{}", enc_key))
            .await?;

        let assignee_val = if !raw["fields"]["assignee"].is_null() {
            let uname = raw["fields"]["assignee"]["name"].as_str().unwrap_or("");
            let dname = raw["fields"]["assignee"]["displayName"].as_str().unwrap_or("");
            json!({
                "username": uname,
                "displayName": dname,
                "mention_syntax": format!("[~{}]", uname),
            })
        } else {
            Value::Null
        };

        let reporter_val = if !raw["fields"]["reporter"].is_null() {
            let uname = raw["fields"]["reporter"]["name"].as_str().unwrap_or("");
            let dname = raw["fields"]["reporter"]["displayName"].as_str().unwrap_or("");
            json!({
                "username": uname,
                "displayName": dname,
                "mention_syntax": format!("[~{}]", uname),
            })
        } else {
            Value::Null
        };

        let all_comments = raw["fields"]["comment"]["comments"]
            .as_array()
            .cloned()
            .unwrap_or_default();

        let total_comments = all_comments.len();

        let comments_vec: Vec<Value> = if comments_limit > 0 {
            all_comments
                .iter()
                .rev()
                .take(comments_limit as usize)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .map(|c| {
                    let uname = c["author"]["name"].as_str().unwrap_or("");
                    let dname = c["author"]["displayName"].as_str().unwrap_or("");
                    json!({
                        "id": c["id"],
                        "author": {
                            "username": uname,
                            "displayName": dname,
                            "mention_syntax": format!("[~{}]", uname),
                        },
                        "created": c["createdDate"].as_str().or(c["created"].as_str()).unwrap_or(""),
                        "body": c["body"].as_str().unwrap_or(""),
                    })
                })
                .collect()
        } else {
            Vec::new()
        };

        Ok(json!({
            "key": raw["key"],
            "summary": raw["fields"]["summary"],
            "status": raw["fields"]["status"]["name"],
            "issue_type": raw["fields"]["issuetype"]["name"],
            "assignee": assignee_val,
            "reporter": reporter_val,
            "priority": raw["fields"]["priority"]["name"],
            "labels": raw["fields"]["labels"],
            "description": raw["fields"]["description"],
            "comments_count": total_comments,
            "comments": comments_vec,
            "link": format!("{}/browse/{}", self.http.base_url(), raw["key"].as_str().unwrap_or(&key)),
        }))
    }

    /// POST /rest/api/2/issue/{key}/comment (支持直接传入 Issue Key 或网页 URL)
    pub async fn add_comment(&self, key_or_url: &str, text: &str) -> Result<Value> {
        let key = parse_jira_key(key_or_url);
        let enc_key = urlencoding::encode(&key);
        let raw = self
            .http
            .post(
                &format!("/rest/api/2/issue/{}/comment", enc_key),
                json!({ "body": text }),
            )
            .await?;
        Ok(json!({
            "status": "success",
            "issue": key,
            "comment_id": raw["id"],
            "author": raw["author"]["displayName"],
        }))
    }

    /// POST /rest/api/2/issue/{key}/transitions (支持直接传入 Issue Key 或网页 URL)
    pub async fn transition(&self, key_or_url: &str, status: &str) -> Result<Value> {
        let key = parse_jira_key(key_or_url);
        let enc_key = urlencoding::encode(&key);
        let meta = self
            .http
            .get(&format!("/rest/api/2/issue/{}/transitions", enc_key))
            .await?;

        let trans_id = meta["transitions"]
            .as_array()
            .and_then(|arr| {
                arr.iter().find(|t| {
                    t["name"]
                        .as_str()
                        .map(|n| n.eq_ignore_ascii_case(status))
                        .unwrap_or(false)
                })
            })
            .and_then(|t| t["id"].as_str())
            .ok_or_else(|| anyhow::anyhow!("未找到匹配的状态: '{}'。请检查可用流转状态名", status))?;

        self.http
            .post(
                &format!("/rest/api/2/issue/{}/transitions", enc_key),
                json!({ "transition": { "id": trans_id } }),
            )
            .await?;

        Ok(json!({
            "status": "success",
            "issue": key,
            "new_status": status,
        }))
    }

    /// GET /rest/api/2/search?jql={jql}&maxResults={limit}
    pub async fn search_issues(&self, jql: &str, limit: u32) -> Result<Value> {
        let limit_str = limit.to_string();
        let raw = self
            .http
            .get_with_query(
                "/rest/api/2/search",
                &[("jql", jql), ("maxResults", &limit_str)],
            )
            .await?;

        let issues = raw["issues"].as_array().map(|arr| {
            arr.iter().map(|item| {
                json!({
                    "key": item["key"],
                    "summary": item["fields"]["summary"],
                    "status": item["fields"]["status"]["name"],
                    "issue_type": item["fields"]["issuetype"]["name"],
                    "assignee": item["fields"]["assignee"]["displayName"],
                    "priority": item["fields"]["priority"]["name"],
                    "link": format!("{}/browse/{}", self.http.base_url(), item["key"].as_str().unwrap_or("")),
                })
            }).collect::<Vec<_>>()
        }).unwrap_or_default();

        Ok(json!({
            "jql": jql,
            "total": raw["total"],
            "count": issues.len(),
            "issues": issues,
        }))
    }

    /// POST /rest/api/2/issue
    pub async fn create_issue(&self, a: &CreateIssueArgs) -> Result<Value> {
        let mut fields = serde_json::Map::new();
        fields.insert("project".to_string(), json!({ "key": a.project }));
        fields.insert("summary".to_string(), json!(a.summary));
        fields.insert("issuetype".to_string(), json!({ "name": a.issue_type }));

        if let Some(ref desc) = a.description {
            fields.insert("description".to_string(), json!(desc));
        }
        if let Some(ref assignee) = a.assignee {
            let clean = parse_username(assignee);
            fields.insert("assignee".to_string(), json!({ "name": clean }));
        }
        if let Some(ref priority) = a.priority {
            fields.insert("priority".to_string(), json!({ "name": priority }));
        }
        if let Some(ref labels_str) = a.labels {
            let labels_vec: Vec<&str> = labels_str
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect();
            fields.insert("labels".to_string(), json!(labels_vec));
        }

        let body = json!({ "fields": fields });
        let raw = self.http.post("/rest/api/2/issue", body).await?;
        let key = raw["key"].as_str().unwrap_or("").to_string();

        Ok(json!({
            "status": "success",
            "key": key,
            "id": raw["id"],
            "link": format!("{}/browse/{}", self.http.base_url(), key),
        }))
    }

    /// PUT /rest/api/2/issue/{key} (支持直接传入 Issue Key 或网页 URL)
    pub async fn update_issue(&self, a: &UpdateIssueArgs) -> Result<Value> {
        let key = parse_jira_key(&a.key_or_url);
        let enc_key = urlencoding::encode(&key);
        let mut fields = serde_json::Map::new();

        if let Some(ref sum) = a.summary {
            fields.insert("summary".to_string(), json!(sum));
        }
        if let Some(ref desc) = a.description {
            fields.insert("description".to_string(), json!(desc));
        }
        if let Some(ref assignee) = a.assignee {
            let clean = parse_username(assignee);
            fields.insert("assignee".to_string(), json!({ "name": clean }));
        }
        if let Some(ref priority) = a.priority {
            fields.insert("priority".to_string(), json!({ "name": priority }));
        }
        if let Some(ref labels_str) = a.labels {
            let labels_vec: Vec<&str> = labels_str
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect();
            fields.insert("labels".to_string(), json!(labels_vec));
        }

        if fields.is_empty() {
            bail!("未提供任何需要更新的字段 (--summary, --description, --assignee, --priority, --labels)");
        }

        let body = json!({ "fields": fields });
        self.http
            .put(&format!("/rest/api/2/issue/{}", enc_key), body)
            .await?;

        Ok(json!({
            "status": "success",
            "key": key,
            "updated_fields": fields.keys().cloned().collect::<Vec<_>>(),
            "link": format!("{}/browse/{}", self.http.base_url(), key),
        }))
    }

    /// PUT /rest/api/2/issue/{key}/assignee (支持直接传入 Issue Key 或网页 URL，自动剥离 [~...] 装饰)
    pub async fn assign_issue(&self, key_or_url: &str, assignee: &str) -> Result<Value> {
        let key = parse_jira_key(key_or_url);
        let clean_assignee = parse_username(assignee);
        let enc_key = urlencoding::encode(&key);
        let body = json!({ "name": clean_assignee });
        self.http
            .put(&format!("/rest/api/2/issue/{}/assignee", enc_key), body)
            .await?;

        Ok(json!({
            "status": "success",
            "key": key,
            "assignee": clean_assignee,
            "link": format!("{}/browse/{}", self.http.base_url(), key),
        }))
    }

    /// GET /rest/api/2/user/search?username={query}&maxResults={limit}
    pub async fn search_users(&self, query: &str, limit: u32) -> Result<Value> {
        let limit_str = limit.to_string();
        let raw = self
            .http
            .get_with_query(
                "/rest/api/2/user/search",
                &[("username", query), ("maxResults", &limit_str)],
            )
            .await?;

        let q_lower = query.trim().to_lowercase();

        let users = raw.as_array().map(|arr| {
            arr.iter().map(|u| {
                let username = u["name"].as_str().unwrap_or("");
                let display_name = u["displayName"].as_str().unwrap_or("");
                let email = u["emailAddress"].as_str().unwrap_or("");
                let active = u["active"].as_bool().unwrap_or(true);

                let exact_match = username.to_lowercase() == q_lower
                    || display_name.to_lowercase() == q_lower
                    || email.to_lowercase() == q_lower;

                json!({
                    "username": username,
                    "displayName": display_name,
                    "email": email,
                    "active": active,
                    "exact_match": exact_match,
                    "mention_syntax": format!("[~{}]", username),
                })
            }).collect::<Vec<_>>()
        }).unwrap_or_default();

        let has_exact = users.iter().any(|u| u["exact_match"] == true);
        let is_ambiguous = users.len() > 1 && !has_exact;

        Ok(json!({
            "query": query,
            "count": users.len(),
            "has_exact_match": has_exact,
            "is_ambiguous": is_ambiguous,
            "users": users,
        }))
    }

    /// GET /rest/api/2/user/assignable/search?issueKey={key}&username={query}&maxResults={limit}
    pub async fn search_assignable_users(
        &self,
        key_or_url: &str,
        query: Option<&str>,
        limit: u32,
    ) -> Result<Value> {
        let key = parse_jira_key(key_or_url);
        let limit_str = limit.to_string();
        let q = query.unwrap_or("");

        let raw = self
            .http
            .get_with_query(
                "/rest/api/2/user/assignable/search",
                &[
                    ("issueKey", &key),
                    ("username", q),
                    ("maxResults", &limit_str),
                ],
            )
            .await?;

        let q_lower = q.trim().to_lowercase();

        let users = raw.as_array().map(|arr| {
            arr.iter().map(|u| {
                let username = u["name"].as_str().unwrap_or("");
                let display_name = u["displayName"].as_str().unwrap_or("");
                let email = u["emailAddress"].as_str().unwrap_or("");
                let active = u["active"].as_bool().unwrap_or(true);

                let exact_match = !q_lower.is_empty()
                    && (username.to_lowercase() == q_lower
                        || display_name.to_lowercase() == q_lower
                        || email.to_lowercase() == q_lower);

                json!({
                    "username": username,
                    "displayName": display_name,
                    "email": email,
                    "active": active,
                    "exact_match": exact_match,
                    "mention_syntax": format!("[~{}]", username),
                })
            }).collect::<Vec<_>>()
        }).unwrap_or_default();

        let has_exact = users.iter().any(|u| u["exact_match"] == true);
        let is_ambiguous = users.len() > 1 && !has_exact;

        Ok(json!({
            "issue_key": key,
            "query": q,
            "count": users.len(),
            "has_exact_match": has_exact,
            "is_ambiguous": is_ambiguous,
            "assignable_users": users,
        }))
    }

    /// POST /rest/api/2/issue/{key}/worklog (在单子上登记工作工时与日志)
    pub async fn add_worklog(&self, a: &AddWorklogArgs) -> Result<Value> {
        let key = parse_jira_key(&a.key_or_url);
        let path = format!("/rest/api/2/issue/{}/worklog", urlencoding::encode(&key));

        let mut body = json!({
            "timeSpent": a.time_spent.trim(),
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

        let raw = self.http.post(&path, body).await?;

        Ok(json!({
            "status": "success",
            "issue_key": key,
            "worklog_id": raw["id"].as_str().unwrap_or(""),
            "time_spent": raw["timeSpent"].as_str().unwrap_or(&a.time_spent),
            "started": raw["started"].as_str().unwrap_or(""),
            "author": raw["author"]["displayName"].as_str().or(raw["author"]["name"].as_str()).unwrap_or(""),
            "comment": raw["comment"].as_str().unwrap_or(""),
            "url": format!("{}/browse/{}", self.http.base_url(), key),
        }))
    }

    /// GET /rest/api/2/issue/{key}/worklog (查询单子上的历史工时日志记录)
    pub async fn list_worklogs(&self, a: &ListWorklogsArgs) -> Result<Value> {
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
    pub async fn delete_worklog(&self, a: &DeleteWorklogArgs) -> Result<Value> {
        let key = parse_jira_key(&a.key_or_url);
        let path = format!(
            "/rest/api/2/issue/{}/worklog/{}",
            urlencoding::encode(&key),
            urlencoding::encode(a.worklog_id.trim())
        );

        self.http.delete(&path).await?;

        Ok(json!({
            "status": "success",
            "issue_key": key,
            "worklog_id": a.worklog_id.trim(),
            "deleted": true,
            "url": format!("{}/browse/{}", self.http.base_url(), key),
        }))
    }
}

fn format_jira_started_time(s: &str) -> String {
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
