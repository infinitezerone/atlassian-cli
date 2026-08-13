use anyhow::Result;
use serde_json::{json, Value};

use super::cli::{CreatePageArgs, UpdatePageArgs};
use crate::http::HttpClient;
use crate::utils::parse_confluence_id;

/// Confluence 产品客户端
pub struct Confluence {
    http: HttpClient,
}

impl Confluence {
    pub fn new(http: HttpClient) -> Self {
        Self { http }
    }

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

    /// POST /rest/api/content (创建新 Confluence 页面，原生支持时间宏、Jira 卡片宏)
    pub async fn create_page(&self, a: &CreatePageArgs) -> Result<Value> {
        let storage_html = format_to_storage_html(&a.body);
        let mut body_json = json!({
            "type": "page",
            "title": a.title,
            "space": { "key": a.space },
            "body": {
                "storage": {
                    "value": storage_html,
                    "representation": "storage"
                }
            }
        });

        if let Some(ref parent_id) = a.parent_id {
            let clean_parent = parse_confluence_id(parent_id);
            body_json["ancestors"] = json!([{ "id": clean_parent }]);
        }

        let raw = self.http.post("/rest/api/content", body_json).await?;
        let page_id = raw["id"].as_str().unwrap_or("").to_string();

        Ok(json!({
            "status": "success",
            "id": page_id,
            "title": raw["title"],
            "version": raw["version"]["number"],
            "space": a.space,
            "url": format!("{}/pages/viewpage.action?pageId={}", self.http.base_url(), page_id),
        }))
    }

    /// PUT /rest/api/content/{id} (安全更新 Confluence 页面，带 5 重准确性防御体系与版本备份)
    pub async fn update_page(&self, a: &UpdatePageArgs) -> Result<Value> {
        let id = parse_confluence_id(&a.id_or_url);
        let path = format!("/rest/api/content/{}", urlencoding::encode(&id));

        // 1. 获取原页面最新数据与 Version
        let orig = self
            .http
            .get_with_query(&path, &[("expand", "body.storage,version")])
            .await?;

        let orig_title = orig["title"].as_str().unwrap_or("").to_string();
        let orig_version = orig["version"]["number"].as_u64().unwrap_or(1);
        let orig_html = orig["body"]["storage"]["value"]
            .as_str()
            .unwrap_or("")
            .to_string();

        let new_title = a.title.as_deref().unwrap_or(&orig_title);

        // 2. 根据不同的编辑模式计算更新后的 HTML
        let (new_html, mode_desc) = if let (Some(ref find_str), Some(ref replace_str)) = (&a.find, &a.replace) {
            // A. 局部精准替换模式 (二义性校验与唯一匹配防御)
            let count = orig_html.matches(find_str).count();
            if count == 0 {
                let trimmed_find = find_str.trim();
                let count_trimmed = orig_html.matches(trimmed_find).count();
                if count_trimmed == 1 {
                    (orig_html.replace(trimmed_find, replace_str), "local_replace_trimmed".to_string())
                } else if count_trimmed == 0 {
                    anyhow::bail!("❌ 错误: 未在原页面中匹配到目标文本: '{}'。请使用 confluence get 确认最新页面内容", find_str);
                } else {
                    anyhow::bail!("❌ 错误: 目标文本在页面中匹配到了 {} 次 (存在二义性)。请在 --find 中包含更长的前后上下文句段", count_trimmed);
                }
            } else if count > 1 {
                anyhow::bail!("❌ 错误: 目标文本在页面中匹配到了 {} 次 (存在二义性)。请在 --find 中包含更长的前后上下文句段", count);
            } else {
                (orig_html.replace(find_str, replace_str), "local_replace".to_string())
            }
        } else if let Some(ref append_str) = a.append {
            // B. 末尾追加模式
            let append_html = format_to_storage_html(append_str);
            (format!("{}\n{}", orig_html, append_html), "append".to_string())
        } else if let Some(ref prepend_str) = a.prepend {
            // C. 顶部插入模式
            let prepend_html = format_to_storage_html(prepend_str);
            (format!("{}\n{}", prepend_html, orig_html), "prepend".to_string())
        } else if let Some(ref body_str) = a.body {
            // D. 全量覆盖模式
            (format_to_storage_html(body_str), "full_overwrite".to_string())
        } else {
            anyhow::bail!("未提供任何更新内容。请使用 --find 与 --replace (局部替换)、--append (末尾追加)、--prepend (顶部插入) 或 --body (全量更新)");
        };

        // 3. 若为 dry_run 只读预览模式
        if a.dry_run {
            return Ok(json!({
                "status": "dry_run_preview",
                "id": id,
                "title": new_title,
                "current_version": orig_version,
                "next_version": orig_version + 1,
                "mode": mode_desc,
                "find_target": a.find,
                "replace_target": a.replace,
                "orig_chars": orig_html.chars().count(),
                "new_chars": new_html.chars().count(),
                "hint": "只读预览完成。未真正提交修改。去掉 --dry-run 标志以保存修改至 Confluence。"
            }));
        }

        // 4. 提交版本更新 PUT 请求
        let put_body = json!({
            "type": "page",
            "title": new_title,
            "version": { "number": orig_version + 1 },
            "body": {
                "storage": {
                    "value": new_html,
                    "representation": "storage"
                }
            }
        });

        let raw = self.http.put(&path, put_body).await?;
        let next_version = raw["version"]["number"].as_u64().unwrap_or(orig_version + 1);

        Ok(json!({
            "status": "success",
            "id": id,
            "title": raw["title"],
            "version": next_version,
            "previous_version": orig_version,
            "mode": mode_desc,
            "url": format!("{}/pages/viewpage.action?pageId={}", self.http.base_url(), id),
            "note": format!("页面已成功更新至 Version {}。如需撤销修改，可在网页端点击 Page History 还原到 Version {}。", next_version, orig_version),
        }))
    }
}

/// 极简 HTML -> 纯文本: 预处理 Confluence 各种 XML 宏 (日期宏 <time>, 人员提及 <ri:user>, 状态宏等)
fn html_to_text(html: &str) -> String {
    let mut s = preprocess_confluence_macros(html);
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

fn preprocess_confluence_macros(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut i = 0;

    while i < html.len() {
        // 1. 识别 <time ...datetime="2026-08-13"...> 日期宏 -> [Date: 2026-08-13]
        if html[i..].starts_with("<time") || html[i..].starts_with("<TIME") {
            if let Some(end_tag) = html[i..].find('>') {
                let tag_str = &html[i..i + end_tag + 1];
                if let Some(dt) = extract_xml_attr(tag_str, "datetime") {
                    out.push_str(&format!(" [Date: {}] ", dt));
                    i += end_tag + 1;
                    if html[i..].starts_with("</time>") || html[i..].starts_with("</TIME>") {
                        i += 7;
                    }
                    continue;
                }
            }
        }

        // 2. 识别 <ri:user ...ri:username="john.doe"...> 人员提到宏 -> [~john.doe]
        if html[i..].starts_with("<ri:user") || html[i..].starts_with("<RI:USER") {
            if let Some(end_tag) = html[i..].find('>') {
                let tag_str = &html[i..i + end_tag + 1];
                if let Some(uname) = extract_xml_attr(tag_str, "ri:username") {
                    out.push_str(&format!(" [~{}] ", uname));
                    i += end_tag + 1;
                    if html[i..].starts_with("</ri:user>") || html[i..].starts_with("</RI:USER>") {
                        i += 10;
                    }
                    continue;
                }
            }
        }

        // 3. 识别状态宏 <ac:parameter ac:name="title">STATUS</ac:parameter> -> [Status: STATUS]
        if html[i..].starts_with("<ac:parameter ac:name=\"title\">")
            || html[i..].starts_with("<ac:parameter ac:name='title'>")
        {
            let prefix_len = if html[i..].starts_with("<ac:parameter ac:name=\"title\">") {
                "<ac:parameter ac:name=\"title\">".len()
            } else {
                "<ac:parameter ac:name='title'>".len()
            };
            if let Some(end_param) = html[i + prefix_len..].find("</ac:parameter>") {
                let title = &html[i + prefix_len..i + prefix_len + end_param];
                out.push_str(&format!(" [Status: {}] ", title.trim()));
                i += prefix_len + end_param + "</ac:parameter>".len();
                continue;
            }
        }

        // 4. 普通字符原样写入
        let c = html[i..].chars().next().unwrap();
        out.push(c);
        i += c.len_utf8();
    }

    out
}

fn extract_xml_attr<'a>(tag: &'a str, attr_name: &str) -> Option<&'a str> {
    let pattern = format!("{}=\"", attr_name);
    if let Some(pos) = tag.find(&pattern) {
        let val_start = pos + pattern.len();
        if let Some(val_end) = tag[val_start..].find('"') {
            return Some(&tag[val_start..val_start + val_end]);
        }
    }
    let pattern_single = format!("{}='", attr_name);
    if let Some(pos) = tag.find(&pattern_single) {
        let val_start = pos + pattern_single.len();
        if let Some(val_end) = tag[val_start..].find('\'') {
            return Some(&tag[val_start..val_start + val_end]);
        }
    }
    None
}

/// 自动将纯文本/Markdown/HTML 转为 Confluence Storage 格式
fn format_to_storage_html(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.starts_with('<') || trimmed.contains("<p>") || trimmed.contains("<ac:") || trimmed.contains("<time") {
        text.to_string()
    } else {
        let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        lines.iter().map(|l| format!("<p>{}</p>", l)).collect::<Vec<_>>().join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_to_storage_html() {
        let plain = "Hello\nWorld";
        assert_eq!(format_to_storage_html(plain), "<p>Hello</p>\n<p>World</p>");

        let raw_macro = "<ac:structured-macro ac:name=\"jira\"><ac:parameter ac:name=\"key\">PROJ-123</ac:parameter></ac:structured-macro>";
        assert_eq!(format_to_storage_html(raw_macro), raw_macro);

        let date_macro = "<time datetime=\"2026-08-13\"/>";
        assert_eq!(format_to_storage_html(date_macro), date_macro);
    }

    #[test]
    fn test_html_to_text_with_confluence_macros() {
        let html = r#"<p>Release Date: <time datetime="2026-08-15"/></p>
<p>Assignee: <ac:link><ri:user ri:username="john.doe"/></ac:link></p>
<p>Status: <ac:structured-macro ac:name="status"><ac:parameter ac:name="title">IN PROGRESS</ac:parameter></ac:structured-macro></p>"#;

        let plain = html_to_text(html);
        assert!(plain.contains("[Date: 2026-08-15]"));
        assert!(plain.contains("[~john.doe]"));
        assert!(plain.contains("[Status: IN PROGRESS]"));
    }
}
