use serde_json::{json, Value};

use super::cli::{BulkCreateArgs, CloneArgs, CreateIssueArgs, GetIssueArgs, UpdateIssueArgs};
use super::Jira;
use crate::error::AppError;
use crate::utils::{parse_jira_key, parse_username};

impl Jira {
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

    /// PUT /rest/api/2/issue/{key}/assignee (支持直接传入 Issue Key 或网页 URL，自动剥离 [~...] 装饰，支持 unassigned 取消指派)
    pub async fn assign_issue(&self, key_or_url: &str, assignee: &str) -> Result<Value, AppError> {
        let key = parse_jira_key(key_or_url);
        let clean_assignee = parse_username(assignee);
        let enc_key = urlencoding::encode(&key);

        let (body, display_assignee) = if clean_assignee.is_empty()
            || clean_assignee.eq_ignore_ascii_case("unassigned")
            || clean_assignee.eq_ignore_ascii_case("none")
            || clean_assignee == "-1"
        {
            (json!({ "name": "-1" }), "unassigned".to_string())
        } else {
            (json!({ "name": clean_assignee }), clean_assignee.clone())
        };

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
            "assignee": display_assignee,
            "link": format!("{}/browse/{}", self.http.base_url(), key),
        }))
    }
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
