use serde_json::{json, Value};

use super::cli::{
    AddWorklogArgs, BulkCreateArgs, CloneArgs, CreateIssueArgs, DeleteWorklogArgs, GetIssueArgs,
    ListWorklogsArgs, UpdateIssueArgs,
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

    /// PUT /rest/api/2/issue/{key}/comment/{id} (编辑已有评论)
    pub async fn update_comment(
        &self,
        key_or_url: &str,
        comment_id: &str,
        text: &str,
    ) -> Result<Value, AppError> {
        // 提及语法校验:编辑后的内容同样校验
        crate::utils::validate_mentions(text)?;
        let key = parse_jira_key(key_or_url);
        let enc_key = urlencoding::encode(&key);
        let enc_id = urlencoding::encode(comment_id.trim());
        let path = format!("/rest/api/2/issue/{}/comment/{}", enc_key, enc_id);
        let body = json!({ "body": text });
        if self.policy.dry_run {
            return Ok(crate::module::preview_json("jira.comment-update", "PUT", &path, &key, Some(&body), None));
        }
        crate::module::require_confirmed(&self.policy)?;
        let raw = self.http.put(&path, body).await?;
        if crate::module::is_replayed(&raw) {
            return Ok(raw);
        }
        Ok(json!({
            "status": "success",
            "issue": key,
            "comment_id": comment_id.trim(),
            "updated": true,
            "author": raw["author"]["displayName"],
        }))
    }

    /// DELETE /rest/api/2/issue/{key}/comment/{id} (删除评论)
    pub async fn delete_comment(
        &self,
        key_or_url: &str,
        comment_id: &str,
    ) -> Result<Value, AppError> {
        let key = parse_jira_key(key_or_url);
        let enc_key = urlencoding::encode(&key);
        let enc_id = urlencoding::encode(comment_id.trim());
        let path = format!("/rest/api/2/issue/{}/comment/{}", enc_key, enc_id);
        if self.policy.dry_run {
            return Ok(crate::module::preview_json("jira.comment-delete", "DELETE", &path, &key, None, None));
        }
        crate::module::require_confirmed(&self.policy)?;
        let raw = self.http.delete(&path).await?;
        if crate::module::is_replayed(&raw) {
            return Ok(raw);
        }
        Ok(json!({
            "status": "success",
            "issue": key,
            "comment_id": comment_id.trim(),
            "deleted": true,
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

        apply_custom_fields(&mut fields, &a.custom, a.custom_json.as_deref())?;

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

    /// POST /rest/api/2/issue/bulk (一次请求批量创建多个单子,官方支持)
    pub async fn bulk_create_issues(&self, a: &BulkCreateArgs) -> Result<Value, AppError> {
        // 收集标题列表:--summaries 逗号分隔 + --from-file 每行
        let mut summaries: Vec<String> = Vec::new();
        if let Some(s) = &a.summaries {
            for part in s.split(',') {
                let t = part.trim();
                if !t.is_empty() {
                    summaries.push(t.to_string());
                }
            }
        }
        if let Some(f) = &a.from_file {
            let content = std::fs::read_to_string(f)
                .map_err(|e| AppError::param_invalid(format!("读取文件失败 {}: {}", f, e)))?;
            for line in content.lines() {
                let t = line.trim();
                if !t.is_empty() {
                    summaries.push(t.to_string());
                }
            }
        }
        if summaries.is_empty() {
            return Err(AppError::param_invalid(
                "未提供任何单子标题: 请用 --summaries \"a,b,c\" 或 --from-file file.txt",
            ));
        }

        // 共享字段模板(project/type/priority/labels/assignee/custom 等对所有批量单子一致)
        let mut fields = serde_json::Map::new();
        fields.insert("project".to_string(), json!({ "key": a.project }));
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
        apply_custom_fields(&mut fields, &a.custom, a.custom_json.as_deref())?;

        let issue_updates: Vec<Value> = summaries
            .iter()
            .map(|s| {
                let mut f = fields.clone();
                f.insert("summary".to_string(), json!(s));
                json!({ "fields": f })
            })
            .collect();

        let body = json!({ "issueUpdates": issue_updates });
        if self.policy.dry_run {
            return Ok(crate::module::preview_json(
                "jira.bulk-create",
                "POST",
                "/rest/api/2/issue/bulk",
                &format!("{} issues", summaries.len()),
                Some(&body),
                None,
            ));
        }
        crate::module::require_confirmed(&self.policy)?;
        let raw = self.http.post("/rest/api/2/issue/bulk", body).await?;
        if crate::module::is_replayed(&raw) {
            return Ok(raw);
        }

        let issues: Vec<Value> = raw["issues"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .map(|i| {
                        json!({
                            "key": i["key"],
                            "id": i["id"],
                            "link": format!("{}/browse/{}", self.http.base_url(), i["key"].as_str().unwrap_or("")),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        let errors: Vec<Value> = raw["errors"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .map(|e| {
                        json!({
                            "element": e["failedElementNumber"],
                            "errors": e["errors"],
                            "error_messages": e["errorMessages"],
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(json!({
            "status": "success",
            "requested": summaries.len(),
            "created": issues.len(),
            "failed": errors.len(),
            "issues": issues,
            "errors": errors,
            "hint": if errors.is_empty() {
                json!("")
            } else {
                json!(format!("{} 个创建失败,见 errors 字段", errors.len()))
            },
        }))
    }

    /// GET /rest/api/2/issue/{key} + POST /rest/api/2/issue (克隆单子:复制业务字段、重置状态/经办人)
    pub async fn clone_issue(&self, a: &CloneArgs) -> Result<Value, AppError> {
        let src_key = parse_jira_key(&a.source);
        let enc_src = urlencoding::encode(&src_key);

        // 1. 读取源单字段(显式字段列表,避免 *all 超大响应)
        let mut field_list = vec![
            "summary", "description", "issuetype", "priority", "labels",
            "components", "fixVersions", "duedate", "environment",
        ];
        if let Some(extra) = &a.extra_fields {
            for f in extra.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                field_list.push(f);
            }
        }
        let fields_q = field_list.join(",");
        let raw = self
            .http
            .get_with_query(&format!("/rest/api/2/issue/{}", enc_src), &[("fields", &fields_q)])
            .await?;
        let src_fields = &raw["fields"];

        // 2. 构造新单字段:只复制业务字段;status/assignee/reporter/comments/worklog/附件一律重置
        let mut new_fields = serde_json::Map::new();
        let target_project = a
            .project
            .as_deref()
            .unwrap_or(src_fields["project"]["key"].as_str().unwrap_or(""));
        new_fields.insert("project".to_string(), json!({ "key": target_project }));
        new_fields.insert(
            "summary".to_string(),
            json!(a.summary.clone().unwrap_or_else(|| src_fields["summary"]
                .as_str()
                .unwrap_or("")
                .to_string())),
        );
        for f in ["issuetype", "priority"] {
            if let Some(id) = src_fields[f]["id"].as_str() {
                new_fields.insert(f.to_string(), json!({ "id": id }));
            } else if let Some(name) = src_fields[f]["name"].as_str() {
                new_fields.insert(f.to_string(), json!({ "name": name }));
            }
        }
        for f in ["labels", "components", "fixVersions"] {
            if !src_fields[f].is_null() {
                new_fields.insert(f.to_string(), src_fields[f].clone());
            }
        }
        for f in ["description", "duedate", "environment"] {
            if let Some(v) = src_fields[f].as_str() {
                if !v.is_empty() {
                    crate::utils::validate_mentions(v)?;
                    new_fields.insert(f.to_string(), json!(v));
                }
            }
        }
        if let Some(extra) = &a.extra_fields {
            for f in extra.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                if !src_fields[f].is_null() {
                    new_fields.insert(f.to_string(), src_fields[f].clone());
                }
            }
        }

        let body = json!({ "fields": new_fields });
        if self.policy.dry_run {
            return Ok(crate::module::preview_json(
                "jira.clone",
                "POST",
                "/rest/api/2/issue",
                &src_key,
                Some(&body),
                None,
            ));
        }
        crate::module::require_confirmed(&self.policy)?;
        let raw = self.http.post("/rest/api/2/issue", body).await?;
        if crate::module::is_replayed(&raw) {
            return Ok(raw);
        }
        let new_key = raw["key"].as_str().unwrap_or("").to_string();

        // 3. 可选副作用:Cloners 关联 + 源单留痕评论
        let mut cloners_link = false;
        let mut trace_comment = false;
        if a.link {
            let link_body = json!({
                "type": { "name": "Cloners" },
                "inwardIssue": { "key": new_key },
                "outwardIssue": { "key": src_key },
            });
            if self.http.post("/rest/api/2/issueLink", link_body).await.is_ok() {
                cloners_link = true;
            }
        }
        if a.comment {
            let note = format!("此单已克隆为 {}. 由 atlassian-cli jira clone 自动留痕。", new_key);
            if self
                .http
                .post(
                    &format!("/rest/api/2/issue/{}/comment", enc_src),
                    json!({ "body": note }),
                )
                .await
                .is_ok()
            {
                trace_comment = true;
            }
        }

        Ok(json!({
            "status": "success",
            "source": src_key,
            "key": new_key,
            "id": raw["id"],
            "link": format!("{}/browse/{}", self.http.base_url(), new_key),
            "cloners_link": cloners_link,
            "trace_comment": trace_comment,
        }))
    }

    /// GET /rest/api/2/project (列出所有可见项目)
    pub async fn list_projects(&self, query: Option<&str>) -> Result<Value, AppError> {
        let raw = self.http.get("/rest/api/2/project").await?;
        let q = query.map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty());
        let projects: Vec<Value> = raw
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter(|p| {
                        q.as_ref()
                            .map(|kw| {
                                p["key"].as_str().unwrap_or("").to_lowercase().contains(kw)
                                    || p["name"].as_str().unwrap_or("").to_lowercase().contains(kw)
                            })
                            .unwrap_or(true)
                    })
                    .map(|p| {
                        json!({
                            "key": p["key"],
                            "name": p["name"],
                            "project_type": p["projectTypeKey"],
                            "style": p["style"],
                            "url": format!("{}/browse/{}", self.http.base_url(), p["key"].as_str().unwrap_or("")),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(json!({
            "status": "ok",
            "count": projects.len(),
            "projects": projects,
        }))
    }

    /// GET /rest/api/2/issue/createmeta (查询项目可用的 issue 类型,避免猜类型名)
    pub async fn get_issue_types(&self, project: Option<&str>, limit: u32) -> Result<Value, AppError> {
        let raw = match project.map(|p| p.trim()).filter(|p| !p.is_empty()) {
            Some(p) => {
                self.http
                    .get_with_query("/rest/api/2/issue/createmeta", &[("projectKeys", p)])
                    .await?
            }
            None => self.http.get("/rest/api/2/issue/createmeta").await?,
        };

        // 聚合所有项目的 issue 类型(按 name 去重)
        let mut seen = std::collections::HashSet::new();
        let mut types: Vec<Value> = Vec::new();
        if let Some(projects) = raw["projects"].as_array() {
            for pr in projects {
                if let Some(iss) = pr["issuetypes"].as_array() {
                    for t in iss {
                        let name = t["name"].as_str().unwrap_or("");
                        if !name.is_empty() && seen.insert(name.to_string()) {
                            types.push(json!({
                                "id": t["id"],
                                "name": t["name"],
                                "subtask": t["subtask"],
                            }));
                        }
                    }
                }
            }
        }
        types.truncate(limit as usize);
        Ok(json!({
            "status": "ok",
            "project": project,
            "count": types.len(),
            "issue_types": types,
            "hint": "创建/克隆单子时使用 issue_types 中的 name 作为 --issue-type",
        }))
    }

    /// GET/POST/DELETE /rest/api/2/issue/{key}/watchers (查看/添加/移除关注人)
    pub async fn manage_watchers(
        &self,
        key_or_url: &str,
        add: Option<&str>,
        remove: Option<&str>,
    ) -> Result<Value, AppError> {
        let key = parse_jira_key(key_or_url);
        let enc_key = urlencoding::encode(&key);
        let path = format!("/rest/api/2/issue/{}/watchers", enc_key);

        if add.is_some() && remove.is_some() {
            return Err(AppError::param_invalid("不能同时使用 --add 和 --remove"));
        }

        if let Some(u) = add {
            let clean = parse_username(u);
            if clean.is_empty() {
                return Err(AppError::param_invalid("--add 的用户名不能为空"));
            }
            let body = json!(clean); // Jira watchers POST body 为 JSON 字符串
            if self.policy.dry_run {
                return Ok(crate::module::preview_json(
                    "jira.watchers-add", "POST", &path, &key, Some(&body), None,
                ));
            }
            crate::module::require_confirmed(&self.policy)?;
            let raw = self.http.post(&path, body).await?;
            if crate::module::is_replayed(&raw) {
                return Ok(raw);
            }
            return Ok(json!({
                "status": "success",
                "issue": key,
                "action": "add",
                "user": clean,
            }));
        }

        if let Some(u) = remove {
            let clean = parse_username(u);
            if clean.is_empty() {
                return Err(AppError::param_invalid("--remove 的用户名不能为空"));
            }
            let del_path = format!("{}?username={}", path, urlencoding::encode(&clean));
            if self.policy.dry_run {
                return Ok(crate::module::preview_json(
                    "jira.watchers-remove", "DELETE", &del_path, &key, None, None,
                ));
            }
            crate::module::require_confirmed(&self.policy)?;
            let raw = self.http.delete(&del_path).await?;
            if crate::module::is_replayed(&raw) {
                return Ok(raw);
            }
            return Ok(json!({
                "status": "success",
                "issue": key,
                "action": "remove",
                "user": clean,
            }));
        }

        // 读操作:查询关注人
        let raw = self.http.get(&path).await?;
        let watchers: Vec<Value> = raw["watchers"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .map(|w| {
                        json!({
                            "username": w["name"],
                            "display_name": w["displayName"],
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(json!({
            "status": "ok",
            "issue": key,
            "is_watching": raw["isWatching"],
            "watchers_count": raw["watchCount"],
            "watchers": watchers,
        }))
    }

    /// DELETE /rest/api/2/issue/{key}/attachments/{id} (删除附件,支持 ID 或文件名)
    pub async fn delete_attachment(
        &self,
        key_or_url: &str,
        attachment_id_or_name: &str,
    ) -> Result<Value, AppError> {
        let key = parse_jira_key(key_or_url);
        let path = format!("/rest/api/2/issue/{}", urlencoding::encode(&key));

        // 解析附件 ID(支持 ID 或文件名,忽略大小写)
        let raw = self
            .http
            .get_with_query(&path, &[("fields", "attachment")])
            .await?;
        let att_list = raw["fields"]["attachment"].as_array().ok_or_else(|| {
            AppError::not_found(format!("工单 {} 上未找到任何附件", key))
        })?;
        let target_str = attachment_id_or_name.trim();
        let matched = att_list
            .iter()
            .find(|att| {
                let id = att["id"].as_str().unwrap_or("");
                let filename = att["filename"].as_str().unwrap_or("");
                id == target_str || filename.eq_ignore_ascii_case(target_str)
            })
            .ok_or_else(|| {
                AppError::not_found(format!(
                    "工单 {} 上未找到 ID 或文件名为 '{}' 的附件",
                    key, target_str
                ))
            })?;
        let att_id = matched["id"].as_str().unwrap_or("");

        let del_path = format!(
            "/rest/api/2/issue/{}/attachments/{}",
            urlencoding::encode(&key),
            urlencoding::encode(att_id)
        );
        if self.policy.dry_run {
            let body = json!({ "attachment_id": att_id, "filename": matched["filename"] });
            return Ok(crate::module::preview_json(
                "jira.attachment-delete", "DELETE", &del_path, &key, Some(&body), None,
            ));
        }
        crate::module::require_confirmed(&self.policy)?;
        let raw = self.http.delete(&del_path).await?;
        if crate::module::is_replayed(&raw) {
            return Ok(raw);
        }
        Ok(json!({
            "status": "success",
            "issue": key,
            "attachment_id": att_id,
            "filename": matched["filename"],
            "deleted": true,
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

        apply_custom_fields(&mut fields, &a.custom, a.custom_json.as_deref())?;

        if fields.is_empty() {
            return Err(AppError::param_invalid(
                "未提供任何需要更新的字段 (--summary, --description, --assignee, --priority, --labels, --custom, --custom-json)",
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

    /// GET /rest/api/2/field (查询 Jira 字段元数据字典，将 customfield_xxx 翻译为人类可读名称)
    pub async fn list_fields(
        &self,
        query: Option<&str>,
        custom_only: bool,
        limit: u32,
    ) -> Result<Value, AppError> {
        let raw = self.http.get("/rest/api/2/field").await?;
        let fields = filter_fields_json(&raw, query, custom_only, limit);

        Ok(json!({
            "query": query.unwrap_or(""),
            "custom_only": custom_only,
            "count": fields.len(),
            "fields": fields,
        }))
    }

    /// GET /rest/api/2/attachment/{id} 或根据 issue 附件列表匹配下载二进制流保存至本地
    pub async fn download_attachment(
        &self,
        key_or_url: &str,
        attachment_id_or_name: &str,
        output_path: Option<&str>,
    ) -> Result<Value, AppError> {
        let key = parse_jira_key(key_or_url);
        let path = format!("/rest/api/2/issue/{}", urlencoding::encode(&key));

        let raw = self
            .http
            .get_with_query(&path, &[("fields", "attachment")])
            .await?;

        let att_list = raw["fields"]["attachment"].as_array().ok_or_else(|| {
            AppError::not_found(format!("工单 {} 上未找到任何附件", key))
        })?;

        let target_str = attachment_id_or_name.trim();

        // 匹配附件 (按 ID 或文件名，忽略大小写)
        let matched = att_list
            .iter()
            .find(|att| {
                let id = att["id"].as_str().unwrap_or("");
                let filename = att["filename"].as_str().unwrap_or("");
                id == target_str || filename.eq_ignore_ascii_case(target_str)
            })
            .ok_or_else(|| {
                AppError::not_found(format!(
                    "工单 {} 上未找到 ID 或文件名为 '{}' 的附件",
                    key, target_str
                ))
            })?;

        let att_id = matched["id"].as_str().unwrap_or("");
        let filename = matched["filename"].as_str().unwrap_or("attachment");
        let download_url = matched["content"].as_str().ok_or_else(|| {
            AppError::generic("附件元数据中缺少 download content URL")
        })?;

        let bytes = self.http.get_bytes(download_url).await?;

        let save_to = match output_path {
            Some(p) if !p.trim().is_empty() => std::path::PathBuf::from(p.trim()),
            _ => std::path::PathBuf::from(filename),
        };

        if let Some(parent) = save_to.parent() {
            if !parent.as_os_str().is_empty() {
                tokio::fs::create_dir_all(parent).await?;
            }
        }

        tokio::fs::write(&save_to, &bytes).await?;

        let abs_path = std::fs::canonicalize(&save_to)
            .unwrap_or_else(|_| save_to.clone())
            .display()
            .to_string();

        Ok(json!({
            "status": "success",
            "issue_key": key,
            "attachment_id": att_id,
            "filename": filename,
            "size": bytes.len(),
            "saved_path": abs_path,
        }))
    }
}

fn filter_fields_json(raw: &Value, query: Option<&str>, custom_only: bool, limit: u32) -> Vec<Value> {
    let q_lower = query.map(|q| q.trim().to_lowercase()).filter(|q| !q.is_empty());
    raw.as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|f| {
                    let id = f["id"].as_str().unwrap_or("");
                    let name = f["name"].as_str().unwrap_or("");
                    let is_custom = f["custom"].as_bool().unwrap_or(false);

                    if custom_only && !is_custom {
                        return None;
                    }

                    if let Some(ref q) = q_lower {
                        if !id.to_lowercase().contains(q) && !name.to_lowercase().contains(q) {
                            return None;
                        }
                    }

                    let field_type = f["schema"]["type"].as_str().unwrap_or("");
                    let schema_custom = f["schema"]["custom"].as_str().unwrap_or("");

                    Some(json!({
                        "id": id,
                        "name": name,
                        "custom": is_custom,
                        "type": field_type,
                        "schema_custom": schema_custom,
                    }))
                })
                .take(limit as usize)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn apply_custom_fields(
    fields: &mut serde_json::Map<String, Value>,
    custom_pairs: &[String],
    custom_json_str: Option<&str>,
) -> Result<(), AppError> {
    for pair in custom_pairs {
        let trimmed = pair.trim();
        if trimmed.is_empty() {
            continue;
        }
        let (k, v) = trimmed
            .split_once('=')
            .ok_or_else(|| AppError::param_invalid(format!("自定义字段格式必须为 KEY=VAL: '{}'", pair)))?;

        let key = k.trim().to_string();
        let val_str = v.trim();

        // 尝试自动推断基本类型 (数字、布尔值)，否则保留为字符串
        let val_json: Value = if let Ok(num) = val_str.parse::<i64>() {
            json!(num)
        } else if let Ok(num_f) = val_str.parse::<f64>() {
            json!(num_f)
        } else if let Ok(b) = val_str.parse::<bool>() {
            json!(b)
        } else if val_str.starts_with('{') || val_str.starts_with('[') {
            serde_json::from_str(val_str).unwrap_or_else(|_| json!(val_str))
        } else {
            json!(val_str)
        };

        fields.insert(key, val_json);
    }

    if let Some(json_s) = custom_json_str {
        let trimmed = json_s.trim();
        if !trimmed.is_empty() {
            let parsed: Value = serde_json::from_str(trimmed).map_err(|e| {
                AppError::param_invalid(format!("--custom-json 解析失败: {}", e))
            })?;
            if let Some(obj) = parsed.as_object() {
                for (k, v) in obj {
                    fields.insert(k.clone(), v.clone());
                }
            } else {
                return Err(AppError::param_invalid("--custom-json 顶层必须为 JSON Object (如 '{\"customfield_10020\": ...}')"));
            }
        }
    }

    Ok(())
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

    #[test]
    fn test_apply_custom_fields() {
        let mut fields = serde_json::Map::new();
        let pairs = vec![
            "customfield_10020=5".to_string(),
            "customfield_10010=PROJ-10".to_string(),
            "customfield_bool=true".to_string(),
        ];
        let json_extra = Some("{\"customfield_obj\": {\"id\": \"123\"}}");

        apply_custom_fields(&mut fields, &pairs, json_extra).unwrap();

        assert_eq!(fields["customfield_10020"], json!(5));
        assert_eq!(fields["customfield_10010"], json!("PROJ-10"));
        assert_eq!(fields["customfield_bool"], json!(true));
        assert_eq!(fields["customfield_obj"]["id"], json!("123"));
    }

    #[test]
    fn test_filter_fields_json() {
        let raw = json!([
            {
                "id": "summary",
                "name": "Summary",
                "custom": false,
                "schema": { "type": "string", "system": "summary" }
            },
            {
                "id": "customfield_10020",
                "name": "Sprint",
                "custom": true,
                "schema": { "type": "array", "custom": "com.pyxis.greenhopper.jira:gh-sprint" }
            },
            {
                "id": "customfield_10010",
                "name": "Epic Link",
                "custom": true,
                "schema": { "type": "string", "custom": "com.pyxis.greenhopper.jira:gh-epic-link" }
            }
        ]);

        let all = filter_fields_json(&raw, None, false, 10);
        assert_eq!(all.len(), 3);

        let custom = filter_fields_json(&raw, None, true, 10);
        assert_eq!(custom.len(), 2);
        assert_eq!(custom[0]["name"], "Sprint");

        let queried = filter_fields_json(&raw, Some("epic"), false, 10);
        assert_eq!(queried.len(), 1);
        assert_eq!(queried[0]["id"], "customfield_10010");

        let queried_by_id = filter_fields_json(&raw, Some("10020"), false, 10);
        assert_eq!(queried_by_id.len(), 1);
        assert_eq!(queried_by_id[0]["name"], "Sprint");
    }
}
