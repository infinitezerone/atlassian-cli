use anyhow::Result;
use clap::Subcommand;
use serde_json::{json, Value};

use crate::config::Config;
use crate::http::HttpClient;
use crate::module::AtlassianModule;
use crate::utils::parse_confluence_id;

/// Confluence 模块的 CLI 子命令
#[derive(Subcommand)]
pub enum ConfluenceActions {
    /// 全文搜索页面
    Search {
        query: String,
        /// 返回条数上限
        #[arg(long, default_value_t = 10)]
        limit: u32,
    },
    /// 获取页面正文 (默认转纯文本, --raw 输出原始 HTML)
    Get {
        id: String,
        #[arg(long)]
        raw: bool,
        /// 最大输出字符数 (默认 8000，设为 0 表示不限制)
        #[arg(long, default_value_t = 8000)]
        max_chars: usize,
        /// 字符起始偏移量 (用于续读超长文档)
        #[arg(long, default_value_t = 0)]
        offset: usize,
    },
}

/// Confluence 产品客户端
pub struct Confluence {
    http: HttpClient,
}

impl Confluence {

    /// GET /rest/api/content/search?cql=siteSearch~"q"
    pub async fn search(&self, query: &str, limit: u32) -> Result<Value> {
        let cql = format!("siteSearch~\"{}\"", query);
        let limit_str = limit.to_string();
        let raw = self
            .http
            .get_with_query(
                "/rest/api/content/search",
                &[("cql", &cql), ("limit", &limit_str)],
            )
            .await?;

        let results = raw["results"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .map(|item| {
                        let id = item["id"].as_str().unwrap_or("");
                        let title = item["title"].as_str().unwrap_or("");
                        let webui = item["_links"]["webui"].as_str().unwrap_or("");
                        json!({
                            "id": id,
                            "title": title,
                            "type": item["type"],
                            "url": format!("{}{}", self.http.base_url(), webui),
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Ok(json!({ "query": query, "count": results.len(), "results": results }))
    }

    /// GET /rest/api/content/{id}?expand=body.storage,version (支持直接传入 Page ID 或网页 URL，带超长截断与分页)
    pub async fn get_page(
        &self,
        id_or_url: &str,
        raw_html: bool,
        max_chars: usize,
        offset: usize,
    ) -> Result<Value> {
        let id = parse_confluence_id(id_or_url);
        let path = format!("/rest/api/content/{}", urlencoding::encode(&id));
        let raw = self
            .http
            .get_with_query(&path, &[("expand", "body.storage,version")])
            .await?;

        let title = raw["title"].as_str().unwrap_or("").to_string();
        let html = raw["body"]["storage"]["value"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let version = raw["version"]["number"].as_u64().unwrap_or(0);

        let content_str = if raw_html { html } else { html_to_text(&html) };
        let total_chars = content_str.chars().count();

        let sliced: String = if offset >= total_chars {
            String::new()
        } else {
            content_str
                .chars()
                .skip(offset)
                .take(if max_chars == 0 { usize::MAX } else { max_chars })
                .collect()
        };

        let returned_chars = sliced.chars().count();
        let is_truncated = max_chars > 0 && (offset + returned_chars < total_chars);

        let mut res = json!({
            "id": id,
            "title": title,
            "version": version,
            "total_chars": total_chars,
            "returned_chars": returned_chars,
            "offset": offset,
            "is_truncated": is_truncated,
            "url": format!("{}/pages/viewpage.action?pageId={}", self.http.base_url(), id),
        });

        if is_truncated {
            res["hint"] = json!(format!(
                "文档总长 {} 字，本次返回从偏移量 {} 开始的 {} 字。追加 --offset {} 可获取后续内容。",
                total_chars, offset, returned_chars, offset + returned_chars
            ));
        }

        if raw_html {
            res["body_html"] = json!(sliced);
        } else {
            res["body_text"] = json!(sliced);
        }

        Ok(res)
    }
}



impl AtlassianModule for Confluence {
    type Action = ConfluenceActions;

    fn module_name() -> &'static str {
        "confluence"
    }

    fn connect(cfg: &Config) -> Result<Self> {
        crate::config::check_ready(cfg, "confluence")?;
        Ok(Self {
            http: HttpClient::new(
                cfg.confluence_url.clone(),
                &cfg.confluence_token,
                cfg.allow_insecure_certs,
            )?,
        })
    }

    async fn handle(&self, action: ConfluenceActions) -> Result<Value> {
        match action {
            ConfluenceActions::Search { query, limit } => self.search(&query, limit).await,
            ConfluenceActions::Get {
                id,
                raw,
                max_chars,
                offset,
            } => self.get_page(&id, raw, max_chars, offset).await,
        }
    }
}

/// 极简 HTML -> 纯文本:Confluence storage 格式正文转给 AI 阅读
fn html_to_text(html: &str) -> String {
    let mut s = html.to_string();
    for tag in [
        "</p>", "</div>", "</li>", "</tr>", "</h1>", "</h2>", "</h3>", "</h4>", "</h5>", "</h6>",
        "</pre>", "<br", "</table>", "</ul>", "</ol>",
    ] {
        s = s.replace(tag, "\n");
    }
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    let entities = [
        ("&nbsp;", " "),
        ("&amp;", "&"),
        ("&lt;", "<"),
        ("&gt;", ">"),
        ("&quot;", "\""),
        ("&#39;", "'"),
        ("&apos;", "'"),
    ];
    for (k, v) in entities {
        out = out.replace(k, v);
    }
    let lines: Vec<&str> = out.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    lines.join("\n")
}
