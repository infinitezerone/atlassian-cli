use serde_json::{json, Value};

use super::Jira;
use crate::error::AppError;
use crate::utils::{parse_jira_key, parse_username};

impl Jira {
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
}

pub(crate) fn filter_fields_json(raw: &Value, query: Option<&str>, custom_only: bool, limit: u32) -> Vec<Value> {
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

#[cfg(test)]
mod tests {
    use super::*;

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
