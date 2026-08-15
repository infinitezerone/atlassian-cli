use serde_json::{json, Value};

use super::cli::{CommentPrArgs, CreatePrArgs, GetPrArgs, ListPrsArgs};
use crate::error::AppError;
use crate::http::HttpClient;
use crate::module::WritePolicy;
use crate::utils::{parse_bitbucket_pr, parse_bitbucket_repo};

/// Bitbucket 产品客户端
pub struct Bitbucket {
    http: HttpClient,
    policy: WritePolicy,
}

impl Bitbucket {
    pub fn new(http: HttpClient, policy: WritePolicy) -> Self {
        Self { http, policy }
    }

    /// POST /rest/api/1.0/projects/{p}/repos/{r}/pull-requests (支持自动加载网页预设 Reviewer 与手动扩展)
    pub async fn create_pr(&self, a: &CreatePrArgs) -> Result<Value, AppError> {
        let path = format!(
            "/rest/api/1.0/projects/{}/repos/{}/pull-requests",
            urlencoding::encode(&a.project),
            urlencoding::encode(&a.repo)
        );

        let mut reviewer_names: Vec<String> = Vec::new();

        // 1. 自动尝试从网页端的 default-reviewers conditions 获取预设 Reviewer (按源分支与目标分支精确过滤)
        //    dry-run 模式跳过该只读探测,预览中注明"将自动加载网页预设"
        if !a.no_default_reviewers && !self.policy.dry_run {
            let cond_path = format!(
                "/rest/default-reviewers/1.0/projects/{}/repos/{}/conditions",
                urlencoding::encode(&a.project),
                urlencoding::encode(&a.repo)
            );
            if let Ok(cond_raw) = self.http.get(&cond_path).await {
                if let Some(arr) = cond_raw.as_array() {
                    for cond in arr {
                        let source_match = matches_ref_matcher(&cond["sourceRefMatcher"], &a.from);
                        let target_match = matches_ref_matcher(&cond["targetRefMatcher"], &a.to);
                        if source_match && target_match {
                            if let Some(revs) = cond["reviewers"].as_array() {
                                for r in revs {
                                    if let Some(uname) = r["name"].as_str() {
                                        if !uname.is_empty() && !reviewer_names.contains(&uname.to_string()) {
                                            reviewer_names.push(uname.to_string());
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // 2. 追加用户通过 --reviewers 手动传入的评审人
        if let Some(ref rev_str) = a.reviewers {
            for item in rev_str.split(',') {
                let clean = crate::utils::parse_username(item);
                if !clean.is_empty() && !reviewer_names.contains(&clean) {
                    reviewer_names.push(clean);
                }
            }
        }

        let reviewers_payload: Vec<Value> = reviewer_names
            .iter()
            .map(|name| json!({ "user": { "name": name } }))
            .collect();

        let body = json!({
            "title": a.title,
            "description": a.description,
            "fromRef": { "id": format!("refs/heads/{}", a.from) },
            "toRef": { "id": format!("refs/heads/{}", a.to) },
            "reviewers": reviewers_payload,
        });

        let target = format!("{}/{}", a.project, a.repo);
        if self.policy.dry_run {
            let hint = if !a.no_default_reviewers {
                Some("只读预览:网页预设 Reviewer 将在实际执行时自动加载。确认执行请追加 --confirm")
            } else {
                None
            };
            return Ok(crate::module::preview_json(
                "bitbucket.create-pr",
                "POST",
                &path,
                &target,
                Some(&body),
                hint,
            ));
        }
        crate::module::require_confirmed(&self.policy)?;

        let raw = self.http.post(&path, body).await?;
        let res_reviewers = raw["reviewers"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .map(|r| {
                        let uname = r["user"]["name"].as_str().unwrap_or("");
                        let dname = r["user"]["displayName"].as_str().unwrap_or("");
                        json!({
                            "username": uname,
                            "displayName": dname,
                            "mention_syntax": format!("@{{{}}}", uname),
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Ok(json!({
            "status": "success",
            "pr_id": raw["id"],
            "title": raw["title"],
            "state": raw["state"],
            "from": raw["fromRef"]["displayId"],
            "to": raw["toRef"]["displayId"],
            "reviewers_count": res_reviewers.len(),
            "reviewers": res_reviewers,
            "link": raw["links"]["self"][0]["href"],
        }))
    }

    /// GET /rest/api/1.0/projects/{p}/repos/{r}/pull-requests/{id} (支持直接传入完整 PR 网页 URL)
    pub async fn get_pr(&self, a: &GetPrArgs) -> Result<Value, AppError> {
        let (project, repo, pr_id) = parse_bitbucket_pr(&a.id_or_url, a.project.as_deref(), a.repo.as_deref())?;
        crate::utils::ensure_clean_id("Bitbucket PR id", &pr_id)?;
        let path = format!(
            "/rest/api/1.0/projects/{}/repos/{}/pull-requests/{}",
            urlencoding::encode(&project),
            urlencoding::encode(&repo),
            urlencoding::encode(&pr_id)
        );
        let raw = self.http.get(&path).await?;
        Ok(json!({
            "pr_id": raw["id"],
            "title": raw["title"],
            "state": raw["state"],
            "from": raw["fromRef"]["displayId"],
            "to": raw["toRef"]["displayId"],
            "author": raw["author"]["user"]["displayName"],
            "created": raw["createdDate"],
            "link": raw["links"]["self"][0]["href"],
        }))
    }

    /// GET /rest/api/1.0/projects/{p}/repos/{r}/pull-requests/{id}/changes 与 /diff
    pub async fn get_pr_diff(&self, a: &GetPrArgs) -> Result<Value, AppError> {
        let (project, repo, pr_id) = parse_bitbucket_pr(&a.id_or_url, a.project.as_deref(), a.repo.as_deref())?;

        let changes_path = format!(
            "/rest/api/1.0/projects/{}/repos/{}/pull-requests/{}/changes",
            urlencoding::encode(&project),
            urlencoding::encode(&repo),
            urlencoding::encode(&pr_id)
        );
        let changes_raw = self
            .http
            .get_with_query(&changes_path, &[("limit", "100")])
            .await?;

        let files: Vec<Value> = changes_raw["values"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .map(|item| {
                        let path = item["path"]["toString"].as_str().unwrap_or("");
                        let change_type = item["type"].as_str().unwrap_or("");
                        json!({
                            "path": path,
                            "type": change_type,
                            "percent_unchanged": item["percentUnchanged"],
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        let diff_path = format!(
            "/rest/api/1.0/projects/{}/repos/{}/pull-requests/{}/diff",
            urlencoding::encode(&project),
            urlencoding::encode(&repo),
            urlencoding::encode(&pr_id)
        );
        let diff_raw = self.http.get(&diff_path).await;

        let diffs = match diff_raw {
            Ok(raw) => raw["diffs"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .take(20)
                        .map(|d| {
                            let src = d["source"]["toString"].as_str();
                            let dst = d["destination"]["toString"].as_str();
                            let file_name = dst.or(src).unwrap_or("");
                            let hunks = d["hunks"]
                                .as_array()
                                .map(|harr| {
                                    harr.iter()
                                        .map(|h| {
                                            let segments = h["segments"]
                                                .as_array()
                                                .map(|sarr| {
                                                    sarr.iter()
                                                        .map(|seg| {
                                                            let stype = seg["type"].as_str().unwrap_or("");
                                                            let lines: Vec<String> = seg["lines"]
                                                                .as_array()
                                                                .map(|larr| {
                                                                    larr.iter()
                                                                        .filter_map(|l| {
                                                                            l["line"]
                                                                                .as_str()
                                                                                .map(|s| s.to_string())
                                                                        })
                                                                        .collect()
                                                                })
                                                                .unwrap_or_default();
                                                            json!({
                                                                "type": stype,
                                                                "lines_count": lines.len(),
                                                                "snippet": lines.iter().take(15).cloned().collect::<Vec<_>>(),
                                                            })
                                                        })
                                                        .collect::<Vec<_>>()
                                                })
                                                .unwrap_or_default();
                                            json!({
                                                "source_line": h["sourceLine"],
                                                "destination_line": h["destinationLine"],
                                                "segments": segments,
                                            })
                                        })
                                        .collect::<Vec<_>>()
                                })
                                .unwrap_or_default();

                            json!({
                                "file": file_name,
                                "hunks": hunks,
                            })
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
            Err(_) => Vec::new(),
        };

        Ok(json!({
            "pr_id": pr_id,
            "project": project,
            "repo": repo,
            "changed_files_count": files.len(),
            "files": files,
            "diffs": diffs,
        }))
    }

    /// GET /rest/api/1.0/projects/{p}/repos/{r}/pull-requests/{id}/activities
    pub async fn get_pr_comments(&self, a: &GetPrArgs) -> Result<Value, AppError> {
        let (project, repo, pr_id) = parse_bitbucket_pr(&a.id_or_url, a.project.as_deref(), a.repo.as_deref())?;
        let path = format!(
            "/rest/api/1.0/projects/{}/repos/{}/pull-requests/{}/activities",
            urlencoding::encode(&project),
            urlencoding::encode(&repo),
            urlencoding::encode(&pr_id)
        );
        let raw = self
            .http
            .get_with_query(&path, &[("limit", "100")])
            .await?;

        let comments = raw["values"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter(|act| act["action"].as_str() == Some("COMMENTED"))
                    .map(|act| {
                        let c = &act["comment"];
                        let anchor = &act["commentAnchor"];
                        json!({
                            "id": c["id"],
                            "author": c["author"]["displayName"],
                            "text": c["text"],
                            "created": c["createdDate"],
                            "file_path": anchor["path"].as_str(),
                            "line": anchor["line"],
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Ok(json!({
            "pr_id": pr_id,
            "project": project,
            "repo": repo,
            "comments_count": comments.len(),
            "comments": comments,
        }))
    }

    /// POST /rest/api/1.0/projects/{p}/repos/{r}/pull-requests/{id}/comments (支持全局评论与指定文件/行号的行内评论)
    pub async fn add_pr_comment(&self, a: &CommentPrArgs) -> Result<Value, AppError> {
        let (project, repo, pr_id) = parse_bitbucket_pr(&a.id_or_url, a.project.as_deref(), a.repo.as_deref())?;
        let path = format!(
            "/rest/api/1.0/projects/{}/repos/{}/pull-requests/{}/comments",
            urlencoding::encode(&project),
            urlencoding::encode(&repo),
            urlencoding::encode(&pr_id)
        );

        let mut body = json!({ "text": a.text });

        if let Some(ref file_path) = a.file {
            let mut anchor = serde_json::Map::new();
            anchor.insert("path".to_string(), json!(file_path));
            if let Some(line_num) = a.line {
                anchor.insert("line".to_string(), json!(line_num));
                anchor.insert("lineType".to_string(), json!(a.line_type.to_uppercase()));
                anchor.insert("fileType".to_string(), json!(a.file_type.to_uppercase()));
            }
            body["anchor"] = Value::Object(anchor);
        }

        if self.policy.dry_run {
            return Ok(crate::module::preview_json("bitbucket.comment-pr", "POST", &path, &pr_id, Some(&body), None));
        }
        crate::module::require_confirmed(&self.policy)?;

        let raw = self.http.post(&path, body).await?;

        let anchor_info = if !raw["commentAnchor"].is_null() {
            json!({
                "file_path": raw["commentAnchor"]["path"],
                "line": raw["commentAnchor"]["line"],
                "line_type": raw["commentAnchor"]["lineType"],
                "file_type": raw["commentAnchor"]["fileType"],
            })
        } else {
            Value::Null
        };

        Ok(json!({
            "status": "success",
            "comment_id": raw["id"],
            "pr_id": pr_id,
            "project": project,
            "repo": repo,
            "author": raw["author"]["displayName"],
            "text": raw["text"],
            "anchor": anchor_info,
        }))
    }

    /// GET /rest/api/1.0/users?filter={query}&limit={limit}
    pub async fn search_users(&self, query: &str, limit: u32) -> Result<Value, AppError> {
        let limit_str = limit.to_string();
        let raw = self
            .http
            .get_with_query(
                "/rest/api/1.0/users",
                &[("filter", query), ("limit", &limit_str)],
            )
            .await?;

        let q_lower = query.trim().to_lowercase();

        let users = raw["values"].as_array().map(|arr| {
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
                    "mention_syntax": format!("@{{{}}}", username),
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

    /// GET /rest/api/1.0/projects/{p}/repos/{r}/pull-requests?state={state}&limit={limit}
    pub async fn list_prs(&self, a: &ListPrsArgs) -> Result<Value, AppError> {
        let (project, repo) = parse_bitbucket_repo(
            a.url.as_deref(),
            a.project.as_deref(),
            a.repo.as_deref(),
        )?;

        let state_upper = a.state.to_uppercase();
        let limit_str = a.limit.to_string();

        let path = format!(
            "/rest/api/1.0/projects/{}/repos/{}/pull-requests",
            urlencoding::encode(&project),
            urlencoding::encode(&repo)
        );

        let raw = self
            .http
            .get_with_query(
                &path,
                &[("state", &state_upper), ("limit", &limit_str)],
            )
            .await?;

        let prs = raw["values"].as_array().map(|arr| {
            arr.iter().map(|item| {
                let id = item["id"].as_i64().unwrap_or(0);
                let title = item["title"].as_str().unwrap_or("");
                let state = item["state"].as_str().unwrap_or("");
                let author_uname = item["author"]["user"]["name"].as_str().unwrap_or("");
                let author_dname = item["author"]["user"]["displayName"].as_str().unwrap_or("");
                let from_branch = item["fromRef"]["displayId"].as_str().unwrap_or("");
                let to_branch = item["toRef"]["displayId"].as_str().unwrap_or("");
                let created_date = item["createdDate"].as_i64().unwrap_or(0);
                let updated_date = item["updatedDate"].as_i64().unwrap_or(0);

                let web_url = format!(
                    "{}/projects/{}/repos/{}/pull-requests/{}",
                    self.http.base_url(),
                    project,
                    repo,
                    id
                );

                json!({
                    "id": id,
                    "title": title,
                    "state": state,
                    "author": {
                        "username": author_uname,
                        "displayName": author_dname,
                        "mention_syntax": format!("@{{{}}}", author_uname),
                    },
                    "from_branch": from_branch,
                    "to_branch": to_branch,
                    "created_date": created_date,
                    "updated_date": updated_date,
                    "url": web_url,
                })
            }).collect::<Vec<_>>()
        }).unwrap_or_default();

        Ok(json!({
            "project": project,
            "repo": repo,
            "state": state_upper,
            "count": prs.len(),
            "pull_requests": prs,
        }))
    }

    /// POST /rest/api/1.0/projects/{p}/repos/{r}/pull-requests/{id}/approve (支持直接传入网页 URL)
    pub async fn approve_pr(&self, a: &GetPrArgs) -> Result<Value, AppError> {
        let (project, repo, pr_id) = parse_bitbucket_pr(&a.id_or_url, a.project.as_deref(), a.repo.as_deref())?;
        let path = format!(
            "/rest/api/1.0/projects/{}/repos/{}/pull-requests/{}/approve",
            urlencoding::encode(&project),
            urlencoding::encode(&repo),
            urlencoding::encode(&pr_id)
        );
        if self.policy.dry_run {
            return Ok(crate::module::preview_json(
                "bitbucket.approve-pr",
                "POST",
                &path,
                &pr_id,
                Some(&json!({})),
                None,
            ));
        }
        crate::module::require_confirmed(&self.policy)?;
        let raw = self.http.post(&path, json!({})).await?;

        Ok(json!({
            "status": "success",
            "pr_id": pr_id,
            "project": project,
            "repo": repo,
            "approved": true,
            "user": raw["user"]["displayName"].as_str().unwrap_or(""),
            "approved_status": raw["status"].as_str().unwrap_or("APPROVED"),
        }))
    }
}

/// 校验分支名是否符合 Bitbucket Default Reviewers 的 RefMatcher 条件规则
fn matches_ref_matcher(matcher: &Value, branch_name: &str) -> bool {
    let m_type = matcher["type"]["id"].as_str().unwrap_or("");
    let m_id = matcher["id"].as_str().unwrap_or("");
    let m_display = matcher["displayId"].as_str().unwrap_or("");

    match m_type {
        "ANY_REF" => true,
        "BRANCH" => {
            m_id == format!("refs/heads/{}", branch_name) || m_display == branch_name
        }
        "MODEL_CATEGORY" => {
            let cat = m_id.to_lowercase();
            let b_lower = branch_name.to_lowercase();
            b_lower.starts_with(&cat) || b_lower.contains(&format!("/{}/", cat)) || b_lower.contains(&format!("/{}", cat))
        }
        "PATTERN" => {
            if m_display == "*" {
                true
            } else {
                let pat_clean = m_display.replace('*', "");
                branch_name.contains(&pat_clean)
            }
        }
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_matches_ref_matcher() {
        let any_ref = json!({ "type": { "id": "ANY_REF" } });
        assert!(matches_ref_matcher(&any_ref, "feature/test"));

        let branch_ref = json!({
            "type": { "id": "BRANCH" },
            "id": "refs/heads/master",
            "displayId": "master"
        });
        assert!(matches_ref_matcher(&branch_ref, "master"));
        assert!(!matches_ref_matcher(&branch_ref, "release/6.2.0"));

        let model_category = json!({
            "type": { "id": "MODEL_CATEGORY" },
            "id": "RELEASE",
            "displayId": "Release"
        });
        assert!(matches_ref_matcher(&model_category, "release/6.2.0"));
        assert!(!matches_ref_matcher(&model_category, "master"));
    }
}
