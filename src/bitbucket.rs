use anyhow::Result;
use clap::{Args, Subcommand};
use serde_json::{json, Value};

use crate::config::Config;
use crate::http::HttpClient;
use crate::module::AtlassianModule;
use crate::utils::{parse_bitbucket_pr, parse_bitbucket_repo};

#[derive(Args)]
pub struct ListPrsArgs {
    /// Bitbucket Project 名 (若提供 --url 则可省略)
    #[arg(long)]
    pub project: Option<String>,
    /// Bitbucket Repo 名 (若提供 --url 则可省略)
    #[arg(long)]
    pub repo: Option<String>,
    /// 仓库网页 URL (例如 https://bitbucket.example.com/projects/PROJ/repos/my-repo)
    #[arg(long)]
    pub url: Option<String>,
    /// PR 状态 (默认 OPEN，可选 OPEN / MERGED / DECLINED / ALL)
    #[arg(long, default_value = "OPEN")]
    pub state: String,
    /// 最多返回条数 (默认 10)
    #[arg(long, default_value_t = 10)]
    pub limit: u32,
}

#[derive(Args)]
pub struct CommentPrArgs {
    /// Bitbucket Project 名 (若传入完整 PR 网页 URL 则自动从 URL 解析)
    #[arg(long)]
    pub project: Option<String>,
    /// Bitbucket Repo 名 (若传入完整 PR 网页 URL 则自动从 URL 解析)
    #[arg(long)]
    pub repo: Option<String>,
    /// PR ID 或完整 PR 网页 URL (例如 2420 或网页链接)
    pub id_or_url: String,
    /// 评论文本内容
    #[arg(long)]
    pub text: String,
}

/// Bitbucket 模块的 CLI 子命令
#[derive(Subcommand)]
pub enum BitbucketActions {
    /// 查询 Pull Request 列表 (支持 --project --repo 或直接传入仓库网页 URL)
    ListPrs(ListPrsArgs),
    /// 创建 Pull Request
    CreatePr(CreatePrArgs),
    /// 获取 PR 详情 (支持直接传入网页 URL)
    GetPr(GetPrArgs),
    /// 查看 PR 代码修改差异与变动文件 (支持直接传入网页 URL)
    DiffPr(GetPrArgs),
    /// 查看 PR 的评论讨论树与活动记录 (支持直接传入网页 URL)
    CommentsPr(GetPrArgs),
    /// 在 PR 上发表评论 (支持直接传入网页 URL)
    CommentPr(CommentPrArgs),
    /// 按姓名或邮箱模糊搜索同事 (返回 displayName, email 与防误触 @ 语法 mention_syntax)
    User {
        /// 姓名或邮箱关键字 (如 "John" 或 "john.doe@...")
        query: String,
        /// 最多返回条数 (默认 10)
        #[arg(long, default_value_t = 10)]
        limit: u32,
    },
}

#[derive(Args)]
pub struct CreatePrArgs {
    #[arg(long)]
    pub project: String,
    #[arg(long)]
    pub repo: String,
    #[arg(long)]
    pub title: String,
    #[arg(long)]
    pub description: String,
    #[arg(long)]
    pub from: String,
    #[arg(long)]
    pub to: String,
}

#[derive(Args)]
pub struct GetPrArgs {
    /// Bitbucket Project 名 (例如 PROJ，若传入完整 PR 网页 URL 则自动从 URL 解析)
    #[arg(long)]
    pub project: Option<String>,
    /// Bitbucket Repo 名 (例如 my-repo，若传入完整 PR 网页 URL 则自动从 URL 解析)
    #[arg(long)]
    pub repo: Option<String>,
    /// PR ID 或完整 PR 网页 URL (例如 2420 或 https://gitpub.../pull-requests/2420/overview)
    pub id_or_url: String,
}

/// Bitbucket 产品客户端
pub struct Bitbucket {
    http: HttpClient,
}

impl Bitbucket {

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



impl AtlassianModule for Bitbucket {
    type Action = BitbucketActions;

    fn module_name() -> &'static str {
        "bitbucket"
    }

    fn connect(cfg: &Config) -> Result<Self> {
        crate::config::check_ready(cfg, "bitbucket")?;
        Ok(Self {
            http: HttpClient::new(
                cfg.bitbucket_url.clone(),
                &cfg.bitbucket_token,
                cfg.allow_insecure_certs,
            )?,
        })
    }

    async fn handle(&self, action: BitbucketActions) -> Result<Value> {
        match action {
            BitbucketActions::ListPrs(a) => self.list_prs(&a).await,
            BitbucketActions::CreatePr(a) => self.create_pr(&a).await,
            BitbucketActions::GetPr(a) => self.get_pr(&a).await,
            BitbucketActions::DiffPr(a) => self.get_pr_diff(&a).await,
            BitbucketActions::CommentsPr(a) => self.get_pr_comments(&a).await,
            BitbucketActions::CommentPr(a) => self.add_pr_comment(&a).await,
            BitbucketActions::User { query, limit } => self.search_users(&query, limit).await,
        }
    }
}
