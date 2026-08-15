use serde_json::{json, Value};

use super::cli::{
    AddWorklogArgs, CreateIssueArgs, DeleteWorklogArgs, GetIssueArgs, ListWorklogsArgs,
    UpdateIssueArgs,
};
use crate::error::AppError;
use crate::http::HttpClient;
use crate::module::WritePolicy;
use crate::utils::{parse_jira_key, parse_username};

/// Jira 产品客户端:一个方法 = 一个 API,新增 API 就在这加方法
pub struct Jira {
    http: HttpClient,
    policy: WritePolicy,
}

impl Jira {
    pub fn new(http: HttpClient, policy: WritePolicy) -> Self {
        Self { http, policy }
    }

    /// GET /rest/api/2/issue/{key} -> 支持 --raw 原始全量输出与 --fields 自定义字段挑选
    pub async fn get_issue(&self, a: &GetIssueArgs) -> Result<Value, AppError> {
        let key = parse_jira_key(&a.key);
        crate::utils::ensure_clean_id("Jira issue key", &key)?;
        let enc_key = urlencoding::encode(&key);
        let raw = self
            .http
            .get(&format!("/rest/api/2/issue/{}", enc_key))
            .await?;

        if a.raw {
            return Ok(raw);
        }

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

        let comments_vec: Vec<Value> = if a.comments_limit > 0 {
            all_comments
                .iter()
                .rev()
                .take(a.comments_limit as usize)
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

        let mut res = json!({
            "key": raw["key"],
            "summary": raw["fields"]["summary"],
            "status": raw["fields"]["status"]["name"],
            "issue_type": raw["fields"]["issuetype"]["name"],
            "assignee": assignee_val,
            "reporter": reporter_val,
            "priority": raw["fields"]["priority"]["name"],
            "labels": raw["fields"]["labels"],
            "timetracking": raw["fields"]["timetracking"],
            "description": raw["fields"]["description"],
            "comments_count": total_comments,
            "comments": comments_vec,
            "link": format!("{}/browse/{}", self.http.base_url(), raw["key"].as_str().unwrap_or(&key)),
        });

        if let Some(ref extra_fields_str) = a.fields {
            if let Some(obj) = res.as_object_mut() {
                for f in extra_fields_str.split(',') {
                    let clean_field = f.trim();
                    if !clean_field.is_empty() && !obj.contains_key(clean_field) {
                        obj.insert(clean_field.to_string(), raw["fields"][clean_field].clone());
                    }
                }
            }
        }

        Ok(res)
    }

    /// POST /rest/api/2/issue/{key}/comment (支持直接传入 Issue Key 或网页 URL)
    pub async fn add_comment(&self, key_or_url: &str, text: &str) -> Result<Value, AppError> {
        // 提及语法校验:拦 [~xxx] 格式错误,防止 AI 拼错 @人
        crate::utils::validate_mentions(text)?;
        let key = parse_jira_key(key_or_url);
        let enc_key = urlencoding::encode(&key);
        let path = format!("/rest/api/2/issue/{}/comment", enc_key);
        let body = json!({ "body": text });
        if self.policy.dry_run {
            return Ok(crate::module::preview_json("jira.comment", "POST", &path, &key, Some(&body), None));
        }
        crate::module::require_confirmed(&self.policy)?;
        let raw = self.http.post(&path, body).await?;
        if crate::module::is_replayed(&raw) {
            return Ok(raw);
        }
        Ok(json!({
            "status": "success",
            "issue": key,
            "comment_id": raw["id"],
            "author": raw["author"]["displayName"],
        }))
    }

    /// POST /rest/api/2/issue/{key}/transitions (支持直接传入 Issue Key 或网页 URL)
    pub async fn transition(&self, key_or_url: &str, status: &str) -> Result<Value, AppError> {
        let key = parse_jira_key(key_or_url);
        let enc_key = urlencoding::encode(&key);
        let path = format!("/rest/api/2/issue/{}/transitions", enc_key);
        if self.policy.dry_run {
            // 不预发 GET 查询 transition id,仅展示意图
            let intent = json!({ "transition": { "status": status } });
            return Ok(crate::module::preview_json(
                "jira.transition",
                "POST",
                &path,
                &key,
                Some(&intent),
                Some("只读预览:将以目标状态名实时解析 transition id,未真正执行。确认执行请追加 --confirm"),
            ));
        }
        crate::module::require_confirmed(&self.policy)?;
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
            .ok_or_else(|| AppError::not_found(format!("未找到匹配的状态: '{}'。请检查可用流转状态名", status)))?;

        let raw = self
            .http
            .post(
                &path,
                json!({ "transition": { "id": trans_id } }),
            )
            .await?;
        if crate::module::is_replayed(&raw) {
            return Ok(raw);
        }

        Ok(json!({
            "status": "success",
            "issue": key,
            "new_status": status,
        }))
    }

    /// GET /rest/api/2/search?jql={jql}&maxResults={limit}&startAt={start}&fields={fields}
    pub async fn search_issues(
        &self,
        jql: &str,
        limit: u32,
        fields: Option<&str>,
        start_at: u32,
    ) -> Result<Value, AppError> {
        // 本地语法级校验:拦截空查询 / 括号不配对 / 字符串未闭合等 AI 常见低级错误
        crate::utils::validate_jql(jql)?;
        let limit_str = limit.to_string();
        let start_str = start_at.to_string();
        let mut query: Vec<(&str, &str)> = vec![
            ("jql", jql),
            ("maxResults", &limit_str),
            ("startAt", &start_str),
        ];
        if let Some(f) = fields {
            let f = f.trim();
            if !f.is_empty() {
                query.push(("fields", f));
            }
        }
        let raw = self.http.get_with_query("/rest/api/2/search", &query).await?;

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

        let total = raw["total"].as_u64().unwrap_or(0);
        let mut res = json!({
            "jql": jql,
            "total": raw["total"],
            "start_at": start_at,
            "count": issues.len(),
            "issues": issues,
        });
        // 分页提示:还有更多结果时给出翻页指引
        let fetched = start_at as u64 + issues.len() as u64;
        if fetched < total {
            res["hint"] = json!(format!(
                "共 {} 条,本次返回 {} 条。追加 --start-at {} 获取下一页",
                total,
                issues.len(),
                fetched
            ));
        }
        Ok(res)
    }

    /// GET /rest/api/2/jql/autocompletedata —— 查询 JQL 可用字段与函数
    pub async fn suggest_fields(&self) -> Result<Value, AppError> {
        let raw = self.http.get("/rest/api/2/jql/autocompletedata").await?;
        let fields: Vec<Value> = raw["visibleFieldNames"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .map(|f| {
                        json!({
                            "name": f["name"],
                            "value": f["value"],
                            "display_name": f["displayName"],
                            "auto": f["auto"],
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let functions: Vec<Value> = raw["visibleFunctionNames"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .map(|f| {
                        json!({
                            "name": f["name"],
                            "value": f["value"],
                            "display_name": f["displayName"],
                            "is_list": f["isList"],
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(json!({
            "status": "ok",
            "field_count": fields.len(),
            "fields": fields,
            "function_count": functions.len(),
            "functions": functions,
        }))
    }

    /// GET /rest/api/2/jql/autocompletedata/suggestions —— 查询 JQL 字段候选值
    pub async fn suggest_values(
        &self,
        field: &str,
        query: Option<&str>,
        limit: u32,
    ) -> Result<Value, AppError> {
        let limit_str = limit.to_string();
        let mut params: Vec<(&str, &str)> = vec![("fieldName", field), ("maxResults", &limit_str)];
        if let Some(q) = query {
            let q = q.trim();
            if !q.is_empty() {
                params.push(("fieldValue", q));
            }
        }
        let raw = self
            .http
            .get_with_query("/rest/api/2/jql/autocompletedata/suggestions", &params)
            .await?;
        let results: Vec<Value> = raw["results"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .map(|r| {
                        json!({
                            "value": r["value"],
                            "display_name": r["displayName"],
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(json!({
            "status": "ok",
            "field": field,
            "query": query,
            "count": results.len(),
            "results": results,
            "hint": "拼 JQL 时使用 results 中的 value 作为字段值;提及用户时用 atlassian-cli jira user 查询 mention_syntax",
        }))
    }

    /// POST /rest/api/2/issue
    pub async fn create_issue(&self, a: &CreateIssueArgs) -> Result<Value, AppError> {
        let mut fields = serde_json::Map::new();
        fields.insert("project".to_string(), json!({ "key": a.project }));
        fields.insert("summary".to_string(), json!(a.summary));
        fields.insert("issuetype".to_string(), json!({ "name": a.issue_type }));

        if let Some(ref desc) = a.description {
            crate::utils::validate_mentions(desc)?;
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
        if self.policy.dry_run {
            return Ok(crate::module::preview_json("jira.create", "POST", "/rest/api/2/issue", "(new issue)", Some(&body), None));
        }
        crate::module::require_confirmed(&self.policy)?;
        let raw = self.http.post("/rest/api/2/issue", body).await?;
        if crate::module::is_replayed(&raw) {
            return Ok(raw);
        }
        let key = raw["key"].as_str().unwrap_or("").to_string();

        Ok(json!({
            "status": "success",
            "key": key,
            "id": raw["id"],
            "link": format!("{}/browse/{}", self.http.base_url(), key),
        }))
    }

    /// PUT /rest/api/2/issue/{key} (支持直接传入 Issue Key 或网页 URL)
    pub async fn update_issue(&self, a: &UpdateIssueArgs) -> Result<Value, AppError> {
        let key = parse_jira_key(&a.key_or_url);
        let enc_key = urlencoding::encode(&key);
        let mut fields = serde_json::Map::new();

        if let Some(ref sum) = a.summary {
            fields.insert("summary".to_string(), json!(sum));
        }
        if let Some(ref desc) = a.description {
            crate::utils::validate_mentions(desc)?;
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
            return Err(AppError::param_invalid(
                "未提供任何需要更新的字段 (--summary, --description, --assignee, --priority, --labels)",
            ));
        }

        let body = json!({ "fields": fields });
        let path = format!("/rest/api/2/issue/{}", enc_key);
        if self.policy.dry_run {
            return Ok(crate::module::preview_json("jira.update", "PUT", &path, &key, Some(&body), None));
        }
        crate::module::require_confirmed(&self.policy)?;
        let raw = self.http.put(&path, body).await?;
        if crate::module::is_replayed(&raw) {
            return Ok(raw);
        }

        Ok(json!({
            "status": "success",
            "key": key,
            "updated_fields": fields.keys().cloned().collect::<Vec<_>>(),
            "link": format!("{}/browse/{}", self.http.base_url(), key),
        }))
    }

    /// PUT /rest/api/2/issue/{key}/assignee (支持直接传入 Issue Key 或网页 URL，自动剥离 [~...] 装饰)
    pub async fn assign_issue(&self, key_or_url: &str, assignee: &str) -> Result<Value, AppError> {
        let key = parse_jira_key(key_or_url);
        let clean_assignee = parse_username(assignee);
        let enc_key = urlencoding::encode(&key);
        let body = json!({ "name": clean_assignee });
        let path = format!("/rest/api/2/issue/{}/assignee", enc_key);
        if self.policy.dry_run {
            return Ok(crate::module::preview_json("jira.assign", "PUT", &path, &key, Some(&body), None));
        }
        crate::module::require_confirmed(&self.policy)?;
        let raw = self.http.put(&path, body).await?;
        if crate::module::is_replayed(&raw) {
            return Ok(raw);
        }

        Ok(json!({
            "status": "success",
            "key": key,
            "assignee": clean_assignee,
            "link": format!("{}/browse/{}", self.http.base_url(), key),
        }))
    }

    /// GET /rest/api/2/user/search?username={query}&maxResults={limit}
    pub async fn search_users(&self, query: &str, limit: u32) -> Result<Value, AppError> {
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
    ) -> Result<Value, AppError> {
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
    pub async fn add_worklog(&self, a: &AddWorklogArgs) -> Result<Value, AppError> {
        let key = parse_jira_key(&a.key_or_url);
        let path = format!("/rest/api/2/issue/{}/worklog", urlencoding::encode(&key));

        // 字段值校验:拦 AI 常见的工时格式错 / 开始时间错 / 提及语法错
        crate::utils::validate_time_spent(&a.time_spent)?;
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
            "time_spent": raw["timeSpent"].as_str().unwrap_or(&a.time_spent),
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

    /// GET /rest/api/2/issue/{key}/transitions (查询单子当前所有合法的下一步流转动作与目标状态)
    pub async fn get_transitions(&self, key_or_url: &str) -> Result<Value, AppError> {
        let key = parse_jira_key(key_or_url);
        let path = format!("/rest/api/2/issue/{}/transitions", urlencoding::encode(&key));

        let raw = self.http.get(&path).await?;
        let transitions = raw["transitions"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .map(|t| {
                        json!({
                            "id": t["id"].as_str().unwrap_or(""),
                            "name": t["name"].as_str().unwrap_or(""),
                            "to_status": t["to"]["name"].as_str().unwrap_or(""),
                            "to_status_category": t["to"]["statusCategory"]["name"].as_str().unwrap_or(""),
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Ok(json!({
            "issue_key": key,
            "count": transitions.len(),
            "transitions": transitions,
            "url": format!("{}/browse/{}", self.http.base_url(), key),
        }))
    }

    /// POST /rest/api/2/issueLink (建立两个 Jira 工单之间的关联关系)
    pub async fn link_issue(
        &self,
        from_key_or_url: &str,
        to_key_or_url: &str,
        link_type: &str,
        comment: Option<&str>,
    ) -> Result<Value, AppError> {
        let from_key = parse_jira_key(from_key_or_url);
        let to_key = parse_jira_key(to_key_or_url);

        let mut body = json!({
            "type": {
                "name": link_type.trim()
            },
            "inwardIssue": {
                "key": from_key
            },
            "outwardIssue": {
                "key": to_key
            }
        });

        if let Some(c) = comment {
            let trimmed = c.trim();
            if !trimmed.is_empty() {
                body["comment"] = json!({
                    "body": trimmed
                });
            }
        }

        if self.policy.dry_run {
            let target = format!("{} -> {}", from_key, to_key);
            return Ok(crate::module::preview_json(
                "jira.link",
                "POST",
                "/rest/api/2/issueLink",
                &target,
                Some(&body),
                None,
            ));
        }
        crate::module::require_confirmed(&self.policy)?;
        let raw = self.http.post("/rest/api/2/issueLink", body).await?;
        if crate::module::is_replayed(&raw) {
            return Ok(raw);
        }

        Ok(json!({
            "status": "success",
            "type": link_type.trim(),
            "from_issue": from_key,
            "to_issue": to_key,
            "comment": comment.unwrap_or(""),
            "url": format!("{}/browse/{}", self.http.base_url(), from_key),
        }))
    }

    /// GET /rest/api/2/issue/{key}?fields=attachment (查询工单挂载的全部附件列表)
    pub async fn list_attachments(&self, key_or_url: &str) -> Result<Value, AppError> {
        let key = parse_jira_key(key_or_url);
        let path = format!("/rest/api/2/issue/{}", urlencoding::encode(&key));

        let raw = self
            .http
            .get_with_query(&path, &[("fields", "attachment")])
            .await?;

        let attachments = raw["fields"]["attachment"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .map(|att| {
                        json!({
                            "id": att["id"].as_str().unwrap_or(""),
                            "filename": att["filename"].as_str().unwrap_or(""),
                            "size": att["size"].as_u64().unwrap_or(0),
                            "mime_type": att["mimeType"].as_str().unwrap_or(""),
                            "created": att["created"].as_str().unwrap_or(""),
                            "author": att["author"]["displayName"].as_str().or(att["author"]["name"].as_str()).unwrap_or(""),
                            "download_url": att["content"].as_str().unwrap_or(""),
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Ok(json!({
            "issue_key": key,
            "count": attachments.len(),
            "attachments": attachments,
            "url": format!("{}/browse/{}", self.http.base_url(), key),
        }))
    }

    /// POST /rest/api/2/issue/{key}/attachments (上传本地文件到 Jira 工单作为附件)
    pub async fn attach_file(&self, key_or_url: &str, file_path_str: &str) -> Result<Value, AppError> {
        let key = parse_jira_key(key_or_url);
        let path_obj = std::path::Path::new(file_path_str.trim());
        if !path_obj.exists() {
            return Err(AppError::param_invalid(format!("本地文件不存在: {}", file_path_str)));
        }
        let file_name = path_obj
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("attachment")
            .to_string();

        let endpoint = format!("/rest/api/2/issue/{}/attachments", urlencoding::encode(&key));

        if self.policy.dry_run {
            // 预览仅展示文件名与大小,不读取文件内容
            let size = path_obj.metadata().map(|m| m.len()).unwrap_or(0);
            let body = json!({ "files": [format!("{} ({} bytes)", file_name, size)] });
            return Ok(crate::module::preview_json(
                "jira.attach",
                "POST(multipart)",
                &endpoint,
                &key,
                Some(&body),
                None,
            ));
        }
        crate::module::require_confirmed(&self.policy)?;

        let file_bytes = tokio::fs::read(path_obj).await?;
        let part = reqwest::multipart::Part::bytes(file_bytes).file_name(file_name.clone());
        let form = reqwest::multipart::Form::new().part("file", part);

        let raw = self.http.post_multipart(&endpoint, form).await?;

        Ok(json!({
            "status": "success",
            "issue_key": key,
            "filename": file_name,
            "result": raw,
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
