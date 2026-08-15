use serde_json::{json, Value};

use super::cli::{CreatePageArgs, UpdatePageArgs};
use crate::error::AppError;
use crate::http::HttpClient;
use crate::module::WritePolicy;
use crate::utils::parse_confluence_id;

/// Confluence 产品客户端
pub struct Confluence {
    http: HttpClient,
    policy: WritePolicy,
}

impl Confluence {
    pub fn new(http: HttpClient, policy: WritePolicy) -> Self {
        Self { http, policy }
    }

    /// GET /rest/api/content/search?cql=... (支持全文检索或仅按标题精准搜索 --title-only，以及按空间 space 过滤)
    pub async fn search(
        &self,
        query: &str,
        limit: u32,
        title_only: bool,
        space: Option<&str>,
    ) -> Result<Value, AppError> {
        let mut cql = if title_only {
            format!("title ~ \"{}\"", query)
        } else {
            format!("siteSearch ~ \"{}\"", query)
        };

        if let Some(sp) = space {
            let sp_clean = sp.trim();
            if !sp_clean.is_empty() {
                cql = format!("{} and space = \"{}\"", cql, sp_clean);
            }
        }

        let limit_str = limit.to_string();
        let raw = self
            .http
            .get_with_query(
                "/rest/api/content/search",
                &[
                    ("cql", &cql),
                    ("limit", &limit_str),
                    ("expand", "version,space,history.lastUpdated"),
                ],
            )
            .await?;

        let results = raw["results"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .map(|item| {
                        let id = item["id"].as_str().unwrap_or("");
                        let title = item["title"].as_str().unwrap_or("");
                        let space_key = item["space"]["key"].as_str().unwrap_or("");
                        let version = item["version"]["number"].as_u64().unwrap_or(1);
                        let webui = item["_links"]["webui"].as_str().unwrap_or("");
                        let last_updated = item["history"]["lastUpdated"]["when"].as_str().unwrap_or("");
                        json!({
                            "id": id,
                            "title": title,
                            "space": space_key,
                            "version": version,
                            "type": item["type"],
                            "last_updated": last_updated,
                            "url": format!("{}{}", self.http.base_url(), webui),
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Ok(json!({
            "query": query,
            "title_only": title_only,
            "space": space.unwrap_or(""),
            "count": results.len(),
            "results": results
        }))
    }

    /// GET /rest/api/content/{id} (支持直接传入 Page ID 或网页 URL，带 title_only、超长截断与分页)
    pub async fn get_page(
        &self,
        id_or_url: &str,
        title_only: bool,
        raw_html: bool,
        max_chars: usize,
        offset: usize,
    ) -> Result<Value, AppError> {
        let id = parse_confluence_id(id_or_url);
        let path = format!("/rest/api/content/{}", urlencoding::encode(&id));

        if title_only {
            let raw = self
                .http
                .get_with_query(&path, &[("expand", "version,space,history.lastUpdated")])
                .await?;
            let title = raw["title"].as_str().unwrap_or("").to_string();
            let space_key = raw["space"]["key"].as_str().unwrap_or("").to_string();
            let space_name = raw["space"]["name"].as_str().unwrap_or("").to_string();
            let version = raw["version"]["number"].as_u64().unwrap_or(0);
            let updated = raw["history"]["lastUpdated"]["when"].as_str().unwrap_or("");
            let by = raw["history"]["lastUpdated"]["by"]["displayName"]
                .as_str()
                .or(raw["history"]["lastUpdated"]["by"]["username"].as_str())
                .unwrap_or("");

            return Ok(json!({
                "id": id,
                "title": title,
                "space": {
                    "key": space_key,
                    "name": space_name,
                },
                "version": version,
                "last_updated": updated,
                "updated_by": by,
                "url": format!("{}/pages/viewpage.action?pageId={}", self.http.base_url(), id),
            }));
        }

        let raw = self
            .http
            .get_with_query(&path, &[("expand", "body.storage,version,space")])
            .await?;

        let title = raw["title"].as_str().unwrap_or("").to_string();
        let space_key = raw["space"]["key"].as_str().unwrap_or("").to_string();
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
            "space": space_key,
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

    /// GET /rest/api/content/{id}/child/page (获取直接子页面目录清单)
    pub async fn get_children(&self, id_or_url: &str, limit: u32) -> Result<Value, AppError> {
        let id = parse_confluence_id(id_or_url);
        let path = format!("/rest/api/content/{}/child/page", urlencoding::encode(&id));
        let limit_str = limit.to_string();
        let raw = self
            .http
            .get_with_query(&path, &[("limit", &limit_str), ("expand", "version,space")])
            .await?;

        let children = raw["results"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .map(|item| {
                        let child_id = item["id"].as_str().unwrap_or("");
                        let title = item["title"].as_str().unwrap_or("");
                        let space_key = item["space"]["key"].as_str().unwrap_or("");
                        let version = item["version"]["number"].as_u64().unwrap_or(1);
                        let webui = item["_links"]["webui"].as_str().unwrap_or("");
                        json!({
                            "id": child_id,
                            "title": title,
                            "space": space_key,
                            "version": version,
                            "url": format!("{}{}", self.http.base_url(), webui),
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Ok(json!({
            "parent_id": id,
            "count": children.len(),
            "children": children,
            "url": format!("{}/pages/viewpage.action?pageId={}", self.http.base_url(), id),
        }))
    }

    /// GET /rest/api/space (列出或按关键字检索当前用户有权限的 Confluence 空间)
    pub async fn list_spaces(&self, query: Option<&str>, limit: u32) -> Result<Value, AppError> {
        let limit_str = if query.is_some() { "100".to_string() } else { limit.to_string() };
        let raw = self
            .http
            .get_with_query("/rest/api/space", &[("limit", &limit_str), ("status", "current")])
            .await?;

        let q_lower = query.map(|q| q.trim().to_lowercase()).filter(|q| !q.is_empty());

        let spaces = raw["results"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| {
                        let key = item["key"].as_str().unwrap_or("");
                        let name = item["name"].as_str().unwrap_or("");
                        let space_type = item["type"].as_str().unwrap_or("global");
                        let webui = item["_links"]["webui"].as_str().unwrap_or("");

                        if let Some(ref q) = q_lower {
                            if !key.to_lowercase().contains(q) && !name.to_lowercase().contains(q) {
                                return None;
                            }
                        }

                        Some(json!({
                            "key": key,
                            "name": name,
                            "type": space_type,
                            "url": format!("{}{}", self.http.base_url(), webui),
                        }))
                    })
                    .take(limit as usize)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Ok(json!({
            "query": query.unwrap_or(""),
            "count": spaces.len(),
            "spaces": spaces,
        }))
    }

    /// GET /rest/api/content/{id}/child/attachment (查询 Confluence 页面挂载的全部附件列表)
    pub async fn list_attachments(&self, id_or_url: &str, limit: u32) -> Result<Value, AppError> {
        let id = parse_confluence_id(id_or_url);
        let path = format!("/rest/api/content/{}/child/attachment", urlencoding::encode(&id));
        let limit_str = limit.to_string();
        let raw = self
            .http
            .get_with_query(&path, &[("limit", &limit_str), ("expand", "version")])
            .await?;

        let attachments = raw["results"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .map(|att| {
                        let att_id = att["id"].as_str().unwrap_or("");
                        let title = att["title"].as_str().unwrap_or("");
                        let media_type = att["metadata"]["mediaType"].as_str().unwrap_or("");
                        let size = att["extensions"]["fileSize"].as_u64().unwrap_or(0);
                        let version = att["version"]["number"].as_u64().unwrap_or(1);
                        let download_link = att["_links"]["download"].as_str().unwrap_or("");
                        json!({
                            "id": att_id,
                            "filename": title,
                            "media_type": media_type,
                            "size": size,
                            "version": version,
                            "download_url": format!("{}{}", self.http.base_url(), download_link),
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Ok(json!({
            "page_id": id,
            "count": attachments.len(),
            "attachments": attachments,
            "url": format!("{}/pages/viewpage.action?pageId={}", self.http.base_url(), id),
        }))
    }

    /// POST /rest/api/content/{id}/child/attachment (上传本地文件到 Confluence 页面作为附件)
    pub async fn attach_file(
        &self,
        id_or_url: &str,
        file_path_str: &str,
        comment: Option<&str>,
    ) -> Result<Value, AppError> {
        let id = parse_confluence_id(id_or_url);
        let path_obj = std::path::Path::new(file_path_str.trim());
        if !path_obj.exists() {
            return Err(AppError::param_invalid(format!("本地文件不存在: {}", file_path_str)));
        }
        let file_name = path_obj
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("attachment")
            .to_string();

        let endpoint = format!("/rest/api/content/{}/child/attachment", urlencoding::encode(&id));

        if self.policy.dry_run {
            let size = path_obj.metadata().map(|m| m.len()).unwrap_or(0);
            let body = json!({ "files": [format!("{} ({} bytes)", file_name, size)] });
            return Ok(crate::module::preview_json(
                "confluence.attach",
                "POST(multipart)",
                &endpoint,
                &id,
                Some(&body),
                None,
            ));
        }
        crate::module::require_confirmed(&self.policy)?;

        let file_bytes = tokio::fs::read(path_obj).await?;
        let part = reqwest::multipart::Part::bytes(file_bytes).file_name(file_name.clone());
        let mut form = reqwest::multipart::Form::new().part("file", part);

        if let Some(c) = comment {
            let trimmed = c.trim();
            if !trimmed.is_empty() {
                form = form.text("comment", trimmed.to_string());
            }
        }

        let raw = self.http.post_multipart(&endpoint, form).await?;

        Ok(json!({
            "status": "success",
            "page_id": id,
            "filename": file_name,
            "result": raw,
            "url": format!("{}/pages/viewpage.action?pageId={}", self.http.base_url(), id),
        }))
    }

    /// POST /rest/api/content (创建新 Confluence 页面，原生支持时间宏、Jira 卡片宏)
    pub async fn create_page(&self, a: &CreatePageArgs) -> Result<Value, AppError> {
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

        if self.policy.dry_run {
            return Ok(crate::module::preview_json(
                "confluence.create",
                "POST",
                "/rest/api/content",
                &a.title,
                Some(&body_json),
                None,
            ));
        }
        crate::module::require_confirmed(&self.policy)?;

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
    pub async fn update_page(&self, a: &UpdatePageArgs) -> Result<Value, AppError> {
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
                    return Err(AppError::param_invalid(format!(
                        "❌ 错误: 未在原页面中匹配到目标文本: '{}'。请使用 confluence get 确认最新页面内容",
                        find_str
                    )));
                } else {
                    return Err(AppError::param_invalid(format!(
                        "❌ 错误: 目标文本在页面中匹配到了 {} 次 (存在二义性)。请在 --find 中包含更长的前后上下文句段",
                        count_trimmed
                    )));
                }
            } else if count > 1 {
                return Err(AppError::param_invalid(format!(
                    "❌ 错误: 目标文本在页面中匹配到了 {} 次 (存在二义性)。请在 --find 中包含更长的前后上下文句段",
                    count
                )));
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
            return Err(AppError::param_invalid(
                "未提供任何更新内容。请使用 --find 与 --replace (局部替换)、--append (末尾追加)、--prepend (顶部插入) 或 --body (全量更新)",
            ));
        };

        // 3. 若为 dry_run 只读预览模式 (全局 --dry-run)
        if self.policy.dry_run {
            return Ok(json!({
                "status": "dry_run",
                "action": "confluence.update",
                "method": "PUT",
                "path": path.clone(),
                "target": id.clone(),
                "id": id.clone(),
                "title": new_title,
                "current_version": orig_version,
                "next_version": orig_version + 1,
                "mode": mode_desc,
                "find_target": a.find,
                "replace_target": a.replace,
                "orig_chars": orig_html.chars().count(),
                "new_chars": new_html.chars().count(),
                "hint": "只读预览,未真正提交修改。确认执行请追加 --confirm"
            }));
        }

        // 3.5 写操作确认门禁
        crate::module::require_confirmed(&self.policy)?;

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
