use anyhow::{bail, Result};
use clap::Subcommand;
use serde_json::{json, Value};

use crate::config::Config;
use crate::http::HttpClient;
use crate::module::AtlassianModule;
use crate::utils::{parse_jira_key, parse_username};

#[derive(clap::Args)]
pub struct CreateIssueArgs {
    /// Jira 项目 Key (如 PROJ 或 PROJSA)
    #[arg(long)]
    pub project: String,
    /// 单子标题/概要 (Summary)
    #[arg(long)]
    pub summary: String,
    /// 单子类型 (默认 Task，可选 Bug / Story / Task 等)
    #[arg(long, default_value = "Task")]
    pub issue_type: String,
    /// 单子详细描述 (Description)
    #[arg(long)]
    pub description: Option<String>,
    /// 标签列表 (英文逗号分隔，如 "bug,backend")
    #[arg(long)]
    pub labels: Option<String>,
    /// 指派人用户名 (Assignee username)
    #[arg(long)]
    pub assignee: Option<String>,
    /// 优先级 (Priority，如 High / Medium / Low)
    #[arg(long)]
    pub priority: Option<String>,
}

#[derive(clap::Args)]
pub struct UpdateIssueArgs {
    /// 单子 Key 或网页 URL (如 PROJSA-123 或网页链接)
    pub key_or_url: String,
    /// 新的单子标题/概要 (Summary)
    #[arg(long)]
    pub summary: Option<String>,
    /// 新的单子详细描述 (Description)
    #[arg(long)]
    pub description: Option<String>,
    /// 指派人用户名 (Assignee username)
    #[arg(long)]
    pub assignee: Option<String>,
    /// 优先级 (Priority，如 High / Medium / Low)
    #[arg(long)]
    pub priority: Option<String>,
    /// 标签列表 (英文逗号分隔，如 "bug,backend")
    #[arg(long)]
    pub labels: Option<String>,
}

/// Jira 模块的 CLI 子命令
#[derive(Subcommand)]
pub enum JiraActions {
    /// 查询单子详情 (支持 Key 或网页 URL)
    Get { key: String },
    /// 在单子里加评论
    Comment { key: String, text: String },
    /// 流转单子状态 (按状态名,如 In Progress / Done)
    Transition { key: String, status: String },
    /// JQL 条件搜索单子 (如 "assignee = currentUser() AND status != Closed")
    Search {
        jql: String,
        /// 最多返回条数 (默认 10)
        #[arg(long, default_value_t = 10)]
        limit: u32,
    },
    /// 创建新 Jira 单子
    Create(CreateIssueArgs),
    /// 更新已有 Jira 单子属性 (支持 Key 或网页 URL)
    Update(UpdateIssueArgs),
    /// 快捷指派/变更经办人 (支持 Key 或网页 URL)
    Assign {
        /// 单子 Key 或网页 URL
        key: String,
        /// 经办人用户名 (Assignee username)
        assignee: String,
    },
    /// 按姓名或邮箱模糊搜索同事 (返回 displayName, email 与防误触 @ 语法 mention_syntax)
    User {
        /// 姓名或邮箱关键字 (如 "John" 或 "john.doe@...")
        query: String,
        /// 最多返回条数 (默认 10)
        #[arg(long, default_value_t = 10)]
        limit: u32,
    },
    /// 查询当前单子所有合法且在职的可指派同事列表 (对应网页端 Assignee 输入框提示)
    AssignableUsers {
        /// 单子 Key 或网页 URL
        key: String,
        /// 姓名或邮箱搜索过滤 (可选)
        query: Option<String>,
        /// 最多返回条数 (默认 10)
        #[arg(long, default_value_t = 10)]
        limit: u32,
    },
}

/// Jira 产品客户端:一个方法 = 一个 API,新增 API 就在这加方法
pub struct Jira {
    http: HttpClient,
}

impl Jira {

    /// GET /rest/api/2/issue/{key} -> 裁剪字段 (支持直接传入 Issue Key 或完整网页 URL)
    pub async fn get_issue(&self, key_or_url: &str) -> Result<Value> {
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
        let raw = self
            .http
            .get(&format!("/rest/api/2/issue/{}/transitions", enc_key))
            .await?;
        let transitions = raw["transitions"].as_array().cloned().unwrap_or_default();
        let want = status.to_lowercase();

        let matched = transitions.iter().find(|t| {
            t["name"]
                .as_str()
                .map(|n| n.to_lowercase() == want)
                .unwrap_or(false)
                || t["to"]["name"]
                    .as_str()
                    .map(|n| n.to_lowercase() == want)
                    .unwrap_or(false)
        });

        let Some(t) = matched else {
            let available: Vec<String> = transitions
                .iter()
                .filter_map(|t| t["name"].as_str().map(|s| s.to_string()))
                .collect();
            bail!(
                "状态 '{}' 不可用,该单可用的流转: {}",
                status,
                if available.is_empty() { "无".to_string() } else { available.join(" / ") }
            );
        };

        let tid = t["id"]
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| t["id"].to_string());
        self.http
            .post(
                &format!("/rest/api/2/issue/{}/transitions", enc_key),
                json!({ "transition": { "id": tid } }),
            )
            .await?;
        Ok(json!({ "status": "success", "issue": key, "transition": status }))
    }

    /// GET /rest/api/2/search?jql=...&maxResults=...
    pub async fn search_issues(&self, jql: &str, limit: u32) -> Result<Value> {
        let limit_str = limit.to_string();
        let raw = self
            .http
            .get_with_query(
                "/rest/api/2/search",
                &[("jql", jql), ("maxResults", &limit_str)],
            )
            .await?;

        let issues = raw["issues"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .map(|item| {
                        let key = item["key"].as_str().unwrap_or("");
                        json!({
                            "key": key,
                            "summary": item["fields"]["summary"],
                            "status": item["fields"]["status"]["name"],
                            "issue_type": item["fields"]["issuetype"]["name"],
                            "assignee": item["fields"]["assignee"]["displayName"],
                            "priority": item["fields"]["priority"]["name"],
                            "updated": item["fields"]["updated"],
                            "link": format!("{}/browse/{}", self.http.base_url(), key),
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Ok(json!({
            "jql": jql,
            "count": issues.len(),
            "issues": issues,
        }))
    }

    /// POST /rest/api/2/issue
    /// 参考 jira-operator 组装 Payload: fields = { project: {key}, summary, issuetype: {name}, description, labels, assignee: {name}, priority: {name} }
    pub async fn create_issue(&self, a: &CreateIssueArgs) -> Result<Value> {
        let mut fields = json!({
            "project": { "key": a.project },
            "summary": a.summary,
            "issuetype": { "name": a.issue_type },
        });

        if let Some(ref desc) = a.description {
            fields["description"] = json!(desc);
        }
        if let Some(ref labels_str) = a.labels {
            let labels_vec: Vec<&str> = labels_str
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect();
            fields["labels"] = json!(labels_vec);
        }
        if let Some(ref assignee) = a.assignee {
            fields["assignee"] = json!({ "name": assignee });
        }
        if let Some(ref priority) = a.priority {
            fields["priority"] = json!({ "name": priority });
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
}

impl AtlassianModule for Jira {
    type Action = JiraActions;

    fn module_name() -> &'static str {
        "jira"
    }

    fn connect(cfg: &Config) -> Result<Self> {
        crate::config::check_ready(cfg, "jira")?;
        Ok(Self {
            http: HttpClient::new(
                cfg.jira_url.clone(),
                &cfg.jira_token,
                cfg.allow_insecure_certs,
            )?,
        })
    }

    async fn handle(&self, action: JiraActions) -> Result<Value> {
        match action {
            JiraActions::Get { key } => self.get_issue(&key).await,
            JiraActions::Comment { key, text } => self.add_comment(&key, &text).await,
            JiraActions::Transition { key, status } => self.transition(&key, &status).await,
            JiraActions::Search { jql, limit } => self.search_issues(&jql, limit).await,
            JiraActions::Create(a) => self.create_issue(&a).await,
            JiraActions::Update(a) => self.update_issue(&a).await,
            JiraActions::Assign { key, assignee } => self.assign_issue(&key, &assignee).await,
            JiraActions::User { query, limit } => self.search_users(&query, limit).await,
            JiraActions::AssignableUsers { key, query, limit } => {
                self.search_assignable_users(&key, query.as_deref(), limit).await
            }
        }
    }
}
