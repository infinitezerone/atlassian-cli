use anyhow::Result;
use serde_json::{json, Value};

use super::cli::{CommentPrArgs, CreatePrArgs, GetPrArgs, ListPrsArgs};
use crate::http::HttpClient;
use crate::utils::{parse_bitbucket_pr, parse_bitbucket_repo};

/// Bitbucket 产品客户端
pub struct Bitbucket {
    http: HttpClient,
}

impl Bitbucket {
    pub fn new(http: HttpClient) -> Self {
        Self { http }
    }

    /// POST /rest/api/1.0/projects/{p}/repos/{r}/pull-requests
    pub async fn create_pr(&self, a: &CreatePrArgs) -> Result<Value> {
        let path = format!(
            "/rest/api/1.0/projects/{}/repos/{}/pull-requests",
            urlencoding::encode(&a.project),
            urlencoding::encode(&a.repo)
        );
        let body = json!({
            "title": a.title,
            "description": a.description,
            "fromRef": { "id": format!("refs/heads/{}", a.from) },
            "toRef": { "id": format!("refs/heads/{}", a.to) },
            "reviewers": []
        });
        let raw = self.http.post(&path, body).await?;
        Ok(json!({
            "status": "success",
            "pr_id": raw["id"],
            "title": raw["title"],
            "state": raw["state"],
            "from": raw["fromRef"]["displayId"],
            "to": raw["toRef"]["displayId"],
            "link": raw["links"]["self"][0]["href"],
        }))
    }

    /// GET /rest/api/1.0/projects/{p}/repos/{r}/pull-requests/{id} (支持直接传入完整 PR 网页 URL)
    pub async fn get_pr(&self, a: &GetPrArgs) -> Result<Value> {
        let (project, repo, pr_id) = parse_bitbucket_pr(&a.id_or_url, a.project.as_deref(), a.repo.as_deref())?;
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
    pub async fn get_pr_diff(&self, a: &GetPrArgs) -> Result<Value> {
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
    pub async fn get_pr_comments(&self, a: &GetPrArgs) -> Result<Value> {
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

    /// POST /rest/api/1.0/projects/{p}/repos/{r}/pull-requests/{id}/comments
    pub async fn add_pr_comment(&self, a: &CommentPrArgs) -> Result<Value> {
        let (project, repo, pr_id) = parse_bitbucket_pr(&a.id_or_url, a.project.as_deref(), a.repo.as_deref())?;
        let path = format!(
            "/rest/api/1.0/projects/{}/repos/{}/pull-requests/{}/comments",
            urlencoding::encode(&project),
            urlencoding::encode(&repo),
            urlencoding::encode(&pr_id)
        );
        let body = json!({ "text": a.text });
        let raw = self.http.post(&path, body).await?;

        Ok(json!({
            "status": "success",
            "comment_id": raw["id"],
            "pr_id": pr_id,
            "author": raw["author"]["displayName"],
            "text": raw["text"],
        }))
    }

    /// GET /rest/api/1.0/users?filter={query}&limit={limit}
    pub async fn search_users(&self, query: &str, limit: u32) -> Result<Value> {
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
    pub async fn list_prs(&self, a: &ListPrsArgs) -> Result<Value> {
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
}
