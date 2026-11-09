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

    /// GET /rest/api/content/search?cql=... (支持全文检索或仅按标题精准搜索 --title-only，以及按空间 space 过滤与分页)
    pub async fn search(
        &self,
        query: &str,
        limit: u32,
        start_at: u32,
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
        let start_str = start_at.to_string();
        let raw = self
            .http
            .get_with_query(
                "/rest/api/content/search",
                &[
                    ("cql", &cql),
                    ("limit", &limit_str),
                    ("start", &start_str),
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
            "start_at": start_at,
            "count": results.len(),
            "results": results
        }))
    }

    /// GET /rest/api/content/{id} (支持直接传入 Page ID 或网页 URL，带 title_only、section/table 语义投影、超长截断与分页)
    pub async fn get_page(
        &self,
        id_or_url: &str,
        title_only: bool,
        raw_html: bool,
        section: Option<&str>,
        table: Option<&str>,
        max_chars: usize,
        offset: usize,
    ) -> Result<Value, AppError> {
        let id = parse_confluence_id(id_or_url);
        crate::utils::ensure_clean_id("Confluence page id", &id)?;
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

        let full_content = if raw_html { html } else { html_to_markdown(&html) };

        // 语义投影过滤: --section
        let section_content = if let Some(sec) = section {
            match extract_markdown_section(&full_content, sec) {
                Some(s) => s,
                None => {
                    return Err(AppError::param_invalid(format!(
                        "未在页面中找到标题包含 '{}' 的小节 (--section)",
                        sec
                    )));
                }
            }
        } else {
            full_content
        };

        // 语义投影过滤: --table
        let content_str = if let Some(tbl) = table {
            match extract_markdown_table(&section_content, tbl) {
                Some(t) => t,
                None => {
                    return Err(AppError::param_invalid(format!(
                        "未在内容中匹配到 '{}' 的表格 (--table)",
                        tbl
                    )));
                }
            }
        } else {
            section_content
        };

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
            "format": if raw_html { "raw_html" } else { "markdown" },
            "body": sliced,
            "body_text": sliced, // 始终提供统一字段，消除调用方双套逻辑痛点
            "url": format!("{}/pages/viewpage.action?pageId={}", self.http.base_url(), id),
        });

        if raw_html {
            res["body_html"] = json!(sliced);
        }

        if let Some(sec) = section {
            res["section"] = json!(sec);
        }
        if let Some(tbl) = table {
            res["table"] = json!(tbl);
        }

        if is_truncated {
            res["hint"] = json!(format!(
                "内容总长 {} 字，本次返回从偏移量 {} 开始的 {} 字。追加 --offset {} 可获取后续内容，或使用 --max-chars 0 获取全量。",
                total_chars, offset, returned_chars, offset + returned_chars
            ));
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

    /// GET /rest/api/content/{id}/child/attachment 根据页面附件列表匹配下载二进制流保存至本地
    pub async fn download_attachment(
        &self,
        id_or_url: &str,
        attachment_id_or_name: &str,
        output_path: Option<&str>,
    ) -> Result<Value, AppError> {
        let id = parse_confluence_id(id_or_url);
        let path = format!("/rest/api/content/{}/child/attachment", urlencoding::encode(&id));

        let raw = self
            .http
            .get_with_query(&path, &[("limit", "100"), ("expand", "version")])
            .await?;

        let att_list = raw["results"].as_array().ok_or_else(|| {
            AppError::not_found(format!("页面 {} 上未找到任何附件", id))
        })?;

        let target_str = attachment_id_or_name.trim();

        // 匹配附件 (按 ID 或文件名，忽略大小写)
        let matched = att_list
            .iter()
            .find(|att| {
                let att_id = att["id"].as_str().unwrap_or("");
                let title = att["title"].as_str().unwrap_or("");
                att_id == target_str || title.eq_ignore_ascii_case(target_str)
            })
            .ok_or_else(|| {
                AppError::not_found(format!(
                    "页面 {} 上未找到 ID 或文件名为 '{}' 的附件",
                    id, target_str
                ))
            })?;

        let att_id = matched["id"].as_str().unwrap_or("");
        let filename = matched["title"].as_str().unwrap_or("attachment");
        let download_link = matched["_links"]["download"].as_str().ok_or_else(|| {
            AppError::generic("附件元数据中缺少 download 链接")
        })?;

        let bytes = self.http.get_bytes(download_link).await?;

        let save_to = match output_path {
            Some(p) if !p.trim().is_empty() => std::path::PathBuf::from(p.trim()),
            _ => {
                let safe_name = std::path::Path::new(filename)
                    .file_name()
                    .unwrap_or_else(|| std::ffi::OsStr::new("attachment"));
                std::path::PathBuf::from(safe_name)
            }
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
            "page_id": id,
            "attachment_id": att_id,
            "filename": filename,
            "size": bytes.len(),
            "saved_path": abs_path,
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
        if crate::module::is_replayed(&raw) {
            return Ok(raw);
        }
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
        let (new_html, mode_desc, diff_details) = if let Some(ref set_cell_args) = a.set_cell {
            // A. 表格单元格原子更新模式 (ROW_ANCHOR, COL, VALUE)
            if set_cell_args.len() != 3 {
                return Err(AppError::param_invalid(
                    "--set-cell 参数必须包含 3 个值: <ROW_ANCHOR> <COL> <VALUE>",
                ));
            }
            let (html, diff) = table_set_cell(
                &orig_html,
                &set_cell_args[0],
                &set_cell_args[1],
                &set_cell_args[2],
            )?;
            (html, "set_cell".to_string(), diff)
        } else if let Some(ref append_row_args) = a.append_row {
            // B. 表格末尾增行模式 (TABLE_ANCHOR, CELLS...)
            if append_row_args.len() < 2 {
                return Err(AppError::param_invalid(
                    "--append-row 参数必须至少包含 2 个值: <TABLE_ANCHOR> <CELL_VALUES...>",
                ));
            }
            let (html, diff) = table_append_row(
                &orig_html,
                &append_row_args[0],
                &append_row_args[1..],
            )?;
            (html, "append_row".to_string(), diff)
        } else if let (Some(ref find_str), Some(ref replace_str)) = (&a.find, &a.replace) {
            // C. 局部精准替换模式 (二义性校验与唯一匹配防御)
            let count = orig_html.matches(find_str).count();
            let (html, mode) = if count == 0 {
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
            };
            let diff = json!({
                "find": find_str,
                "replace": replace_str,
            });
            (html, mode, diff)
        } else if let Some(ref append_str) = a.append {
            // D. 末尾追加模式
            let append_html = format_to_storage_html(append_str);
            let diff = json!({ "appended": append_str });
            (format!("{}\n{}", orig_html, append_html), "append".to_string(), diff)
        } else if let Some(ref prepend_str) = a.prepend {
            // E. 顶部插入模式
            let prepend_html = format_to_storage_html(prepend_str);
            let diff = json!({ "prepended": prepend_str });
            (format!("{}\n{}", prepend_html, orig_html), "prepend".to_string(), diff)
        } else if let Some(ref body_str) = a.body {
            // F. 全量覆盖模式
            let full_html = format_to_storage_html(body_str);
            let diff = json!({ "mode": "full_overwrite" });
            (full_html, "full_overwrite".to_string(), diff)
        } else {
            return Err(AppError::param_invalid(
                "未提供任何更新内容。请使用 --set-cell (表格单元格修改)、--append-row (表格增行)、--find 与 --replace (局部替换)、--append (末尾追加)、--prepend (顶部插入) 或 --body (全量更新)",
            ));
        };

        // 3. 若为 dry_run 只读预览模式 (全局 --dry-run)
        if self.policy.dry_run {
            let mut v = crate::module::preview_json(
                "confluence.update",
                "PUT",
                &path,
                &id,
                Some(&diff_details),
                Some("只读预览,未真正提交修改。确认执行请追加 --confirm"),
            );
            // 扩展:更新类预览附带 diff 细节字段
            v["id"] = json!(id.clone());
            v["title"] = json!(new_title);
            v["current_version"] = json!(orig_version);
            v["next_version"] = json!(orig_version + 1);
            v["mode"] = json!(mode_desc);
            v["diff"] = diff_details;
            v["orig_chars"] = json!(orig_html.chars().count());
            v["new_chars"] = json!(new_html.chars().count());
            return Ok(v);
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
        if crate::module::is_replayed(&raw) {
            return Ok(raw);
        }
        let next_version = raw["version"]["number"].as_u64().unwrap_or(orig_version + 1);

        Ok(json!({
            "status": "success",
            "id": id,
            "title": raw["title"],
            "version": next_version,
            "previous_version": orig_version,
            "mode": mode_desc,
            "diff": diff_details,
            "url": format!("{}/pages/viewpage.action?pageId={}", self.http.base_url(), id),
            "note": format!("页面已成功更新至 Version {}。如需撤销修改，可在网页端点击 Page History 还原到 Version {}。", next_version, orig_version),
        }))
    }
}

/// Confluence Storage HTML -> 结构化 Markdown 转换器 (保留表格、标题、列表、代码块与 Confluence 专属宏)
#[allow(dead_code)]
pub(crate) fn html_to_text(html: &str) -> String {
    html_to_markdown(html)
}

pub(crate) fn html_to_markdown(html: &str) -> String {
    // 1. 预处理 Confluence XML 宏 (日期、用户、状态、Jira、面板、折叠)
    let s = preprocess_confluence_macros(html);

    // 2. 抽取代码块并用占位符保护
    let mut code_blocks = Vec::new();
    let s = extract_code_blocks(&s, &mut code_blocks);

    // 3. 抽取表格并转换为 Markdown 管道表格 (占位符保护)
    let mut table_blocks = Vec::new();
    let s = extract_tables(&s, &mut table_blocks);

    // 4. 转换标题 (<h1> - <h6>)
    let mut s = convert_headings(&s);

    // 5. 转换列表 (有序列表与无序列表)
    s = convert_lists(&s);

    // 6. 转换引用块 (<blockquote>)
    s = convert_blockquotes(&s);

    // 7. 转换分割线
    s = s.replace("<hr/>", "\n\n---\n\n")
        .replace("<hr />", "\n\n---\n\n")
        .replace("<hr>", "\n\n---\n\n");

    // 8. 转换超链接
    s = convert_links(&s);

    // 9. 转换行内修饰 (粗体、代码、斜体、删除线)
    s = s.replace("<b>", "**")
        .replace("</b>", "**")
        .replace("<strong>", "**")
        .replace("</strong>", "**")
        .replace("<code>", "`")
        .replace("</code>", "`")
        .replace("<em>", "*")
        .replace("</em>", "*")
        .replace("<i>", "*")
        .replace("</i>", "*")
        .replace("<del>", "~~")
        .replace("</del>", "~~")
        .replace("<s>", "~~")
        .replace("</s>", "~~")
        .replace("<strike>", "~~")
        .replace("</strike>", "~~");

    // 10. 段落与换行
    for tag in ["</p>", "</div>", "</h1>", "</h2>", "</h3>", "</h4>", "</h5>", "</h6>"] {
        s = s.replace(tag, "\n\n");
    }
    s = s.replace("<br/>", "\n")
        .replace("<br />", "\n")
        .replace("<br>", "\n");

    // 11. 剥离剩余无用 HTML/XML 标签
    s = strip_tags(&s);

    // 12. 解码 HTML 实体
    s = decode_html_entities(&s);

    // 13. 还原表格占位符
    for (i, tbl) in table_blocks.into_iter().enumerate() {
        let placeholder = format!("___CONFLUENCE_TABLE_BLOCK_{}___", i);
        s = s.replace(&placeholder, &tbl);
    }

    // 14. 还原代码块占位符
    for (i, code) in code_blocks.into_iter().enumerate() {
        let placeholder = format!("___CONFLUENCE_CODE_BLOCK_{}___", i);
        s = s.replace(&placeholder, &code);
    }

    // 15. 清洗多余空行与空白
    normalize_markdown(&s)
}

fn preprocess_confluence_macros(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut i = 0;
    let s_lower = html.to_lowercase();

    while i < html.len() {
        // 1. <time datetime="YYYY-MM-DD"...>
        if s_lower[i..].starts_with("<time") {
            let next_byte = html.as_bytes().get(i + 5);
            if next_byte == Some(&b'>') || next_byte == Some(&b' ') {
                if let Some(end_tag) = html[i..].find('>') {
                    let tag_str = &html[i..i + end_tag + 1];
                    if let Some(dt) = extract_tag_attr(tag_str, "datetime") {
                        out.push_str(&format!(" [Date: {}] ", dt));
                        i += end_tag + 1;
                        if s_lower[i..].starts_with("</time>") {
                            i += "</time>".len();
                        }
                        continue;
                    }
                }
            }
        }

        // 2. Confluence Link: <ac:link>...</ac:link> (用户提及链接或页面跳转链接)
        if s_lower[i..].starts_with("<ac:link") {
            if let Some(link_end) = s_lower[i..].find("</ac:link>") {
                let link_content = &html[i..i + link_end + "</ac:link>".len()];
                let link_lower = link_content.to_lowercase();

                // 2.1 用户提及: <ac:link><ri:user .../></ac:link>
                if let Some(user_pos) = link_lower.find("<ri:user") {
                    let user_tag_str = &link_content[user_pos..];
                    // 优先提取 link-body 中的展示名 (例如 <ac:plain-text-link-body><![CDATA[张三]]></ac:plain-text-link-body>)
                    let display_name = extract_cdata_or_link_body(link_content);
                    if let Some(dname) = display_name.filter(|d| !d.trim().is_empty()) {
                        out.push_str(&format!("[~{}]", dname.trim()));
                        i += link_content.len();
                        continue;
                    }

                    // 次优提取 ri:username
                    if let Some(uname) = extract_tag_attr(user_tag_str, "ri:username") {
                        out.push_str(&format!("[~{}]", uname));
                        i += link_content.len();
                        continue;
                    }

                    // 保底提取 ri:userkey (例如 ri:userkey="40288082...")
                    if let Some(ukey) = extract_tag_attr(user_tag_str, "ri:userkey") {
                        out.push_str(&format!("[~user:{}]", ukey));
                        i += link_content.len();
                        continue;
                    }
                }

                // 2.2 页面链接: <ac:link><ri:page ri:content-title="..."/></ac:link>
                if let Some(page_pos) = link_lower.find("<ri:page") {
                    let page_str = &link_content[page_pos..];
                    if let Some(title) = extract_tag_attr(page_str, "ri:content-title") {
                        out.push_str(&format!("[{}]", title));
                        i += link_content.len();
                        continue;
                    }
                }
            }
        }

        // 3. 独立 <ri:user ri:username="..." 或 ri:userkey="..."/>
        if s_lower[i..].starts_with("<ri:user") {
            let next_byte = html.as_bytes().get(i + 8);
            if next_byte == Some(&b'>') || next_byte == Some(&b' ') || next_byte == Some(&b'/') {
                if let Some(end_tag) = html[i..].find('>') {
                    let tag_str = &html[i..i + end_tag + 1];
                    if let Some(uname) = extract_tag_attr(tag_str, "ri:username") {
                        out.push_str(&format!("[~{}]", uname));
                        i += end_tag + 1;
                        if s_lower[i..].starts_with("</ri:user>") {
                            i += "</ri:user>".len();
                        }
                        continue;
                    }
                    if let Some(ukey) = extract_tag_attr(tag_str, "ri:userkey") {
                        out.push_str(&format!("[~user:{}]", ukey));
                        i += end_tag + 1;
                        if s_lower[i..].starts_with("</ri:user>") {
                            i += "</ri:user>".len();
                        }
                        continue;
                    }
                }
            }
        }

        // 4. Structured macros: status, jira, info, warning, note, tip, expand
        if s_lower[i..].starts_with("<ac:structured-macro") {
            if let Some(macro_end) = s_lower[i..].find("</ac:structured-macro>") {
                let full_macro = &html[i..i + macro_end + "</ac:structured-macro>".len()];
                if let Some(open_tag_end) = full_macro.find('>') {
                    let open_tag = &full_macro[..open_tag_end + 1];
                    let macro_name = extract_tag_attr(open_tag, "ac:name").unwrap_or("").to_lowercase();

                    match macro_name.as_str() {
                        "status" => {
                            let title = extract_macro_param(full_macro, "title").unwrap_or("");
                            out.push_str(&format!(" [Status: {}] ", title.trim()));
                            i += full_macro.len();
                            continue;
                        }
                        "jira" => {
                            let key = extract_macro_param(full_macro, "key").unwrap_or("");
                            if !key.is_empty() {
                                out.push_str(&format!(" [{}] ", key.trim()));
                                i += full_macro.len();
                                continue;
                            }
                        }
                        "info" | "warning" | "note" | "tip" => {
                            let body = extract_rich_text_body(full_macro).unwrap_or("");
                            let label = match macro_name.as_str() {
                                "warning" => "Warning",
                                "note" => "Note",
                                "tip" => "Tip",
                                _ => "Info",
                            };
                            out.push_str(&format!("\n\n> **[{}]** {}\n\n", label, body.trim()));
                            i += full_macro.len();
                            continue;
                        }
                        "expand" => {
                            let title = extract_macro_param(full_macro, "title").unwrap_or("Expand");
                            let body = extract_rich_text_body(full_macro).unwrap_or("");
                            out.push_str(&format!("\n\n**[Expand: {}]**\n{}\n\n", title.trim(), body.trim()));
                            i += full_macro.len();
                            continue;
                        }
                        _ => {}
                    }
                }
            }
        }

        let c = html[i..].chars().next().unwrap();
        out.push(c);
        i += c.len_utf8();
    }

    out
}

fn extract_tag_attr<'a>(tag: &'a str, attr_name: &str) -> Option<&'a str> {
    let tag_lower = tag.to_lowercase();
    let pattern_double = format!("{}=\"", attr_name.to_lowercase());
    if let Some(pos) = tag_lower.find(&pattern_double) {
        let val_start = pos + pattern_double.len();
        if let Some(val_end) = tag[val_start..].find('"') {
            return Some(&tag[val_start..val_start + val_end]);
        }
    }
    let pattern_single = format!("{}='", attr_name.to_lowercase());
    if let Some(pos) = tag_lower.find(&pattern_single) {
        let val_start = pos + pattern_single.len();
        if let Some(val_end) = tag[val_start..].find('\'') {
            return Some(&tag[val_start..val_start + val_end]);
        }
    }
    None
}

fn extract_macro_param<'a>(macro_str: &'a str, param_name: &str) -> Option<&'a str> {
    let lower = macro_str.to_lowercase();
    let pat1 = format!("<ac:parameter ac:name=\"{}\">", param_name.to_lowercase());
    let pat2 = format!("<ac:parameter ac:name='{}'>", param_name.to_lowercase());
    let (pos, pat_len) = if let Some(p) = lower.find(&pat1) {
        (p, pat1.len())
    } else if let Some(p) = lower.find(&pat2) {
        (p, pat2.len())
    } else {
        return None;
    };
    let content_start = pos + pat_len;
    if let Some(end) = lower[content_start..].find("</ac:parameter>") {
        Some(&macro_str[content_start..content_start + end])
    } else {
        None
    }
}

fn extract_rich_text_body<'a>(macro_str: &'a str) -> Option<&'a str> {
    let lower = macro_str.to_lowercase();
    if let Some(start) = lower.find("<ac:rich-text-body>") {
        let content_start = start + "<ac:rich-text-body>".len();
        if let Some(end) = lower[content_start..].find("</ac:rich-text-body>") {
            return Some(&macro_str[content_start..content_start + end]);
        }
    }
    None
}

fn extract_cdata_or_body<'a>(macro_str: &'a str) -> Option<&'a str> {
    if let Some(cdata_start) = macro_str.find("<![CDATA[") {
        let start = cdata_start + "<![CDATA[".len();
        if let Some(cdata_end) = macro_str[start..].find("]]>") {
            return Some(&macro_str[start..start + cdata_end]);
        }
    }
    let lower = macro_str.to_lowercase();
    if let Some(body_start) = lower.find("<ac:plain-text-body>") {
        let start = body_start + "<ac:plain-text-body>".len();
        if let Some(body_end) = lower[start..].find("</ac:plain-text-body>") {
            return Some(&macro_str[start..start + body_end]);
        }
    }
    None
}

fn extract_cdata_or_link_body<'a>(link_content: &'a str) -> Option<&'a str> {
    if let Some(cdata_start) = link_content.find("<![CDATA[") {
        let start = cdata_start + "<![CDATA[".len();
        if let Some(cdata_end) = link_content[start..].find("]]>") {
            return Some(&link_content[start..start + cdata_end]);
        }
    }
    let lower = link_content.to_lowercase();
    for body_tag in ["<ac:plain-text-link-body>", "<ac:link-body>"] {
        if let Some(start) = lower.find(body_tag) {
            let content_start = start + body_tag.len();
            let close_tag = if body_tag.contains("plain-text") {
                "</ac:plain-text-link-body>"
            } else {
                "</ac:link-body>"
            };
            if let Some(end) = lower[content_start..].find(close_tag) {
                let inner = &link_content[content_start..content_start + end];
                return Some(inner);
            }
        }
    }
    None
}

fn extract_markdown_section(md: &str, query: &str) -> Option<String> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return None;
    }

    let lines: Vec<&str> = md.lines().collect();
    let mut start_idx = None;
    let mut heading_level = 0;

    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            let level = trimmed.chars().take_while(|&c| c == '#').count();
            let heading_text = trimmed[level..].trim().to_lowercase();
            if heading_text.contains(&q) {
                start_idx = Some(idx);
                heading_level = level;
                break;
            }
        }
    }

    let start = match start_idx {
        Some(idx) => idx,
        None => return None,
    };

    let mut end = lines.len();
    for (idx, line) in lines.iter().enumerate().skip(start + 1) {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            let level = trimmed.chars().take_while(|&c| c == '#').count();
            // 下一个同级或更高层级标题结束该 section
            if level <= heading_level {
                end = idx;
                break;
            }
        }
    }

    Some(lines[start..end].join("\n").trim().to_string())
}

fn extract_markdown_table(md: &str, query: &str) -> Option<String> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return None;
    }

    let mut tables = Vec::new();
    let mut current_table_lines = Vec::new();
    let mut in_table = false;

    for line in md.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('|') && trimmed.ends_with('|') {
            in_table = true;
            current_table_lines.push(line);
        } else {
            if in_table && !current_table_lines.is_empty() {
                tables.push(current_table_lines.join("\n"));
                current_table_lines.clear();
            }
            in_table = false;
        }
    }
    if in_table && !current_table_lines.is_empty() {
        tables.push(current_table_lines.join("\n"));
    }

    if tables.is_empty() {
        return None;
    }

    // 1-based 数字索引 (如 "1", "2")
    if let Ok(idx) = q.parse::<usize>() {
        if idx > 0 && idx <= tables.len() {
            return Some(tables[idx - 1].clone());
        }
    }

    // 模糊匹配表头或表内文本
    for tbl in &tables {
        if tbl.to_lowercase().contains(&q) {
            return Some(tbl.clone());
        }
    }

    None
}

fn extract_code_blocks(html: &str, blocks: &mut Vec<String>) -> String {
    let mut out = String::with_capacity(html.len());
    let mut i = 0;
    let s_lower = html.to_lowercase();

    while i < html.len() {
        // 1. Confluence structured macro with ac:name="code"
        if s_lower[i..].starts_with("<ac:structured-macro") {
            if let Some(tag_end) = html[i..].find('>') {
                let open_tag = &html[i..i + tag_end + 1];
                if open_tag.to_lowercase().contains("ac:name=\"code\"")
                    || open_tag.to_lowercase().contains("ac:name='code'")
                {
                    if let Some(macro_end) = s_lower[i..].find("</ac:structured-macro>") {
                        let macro_str = &html[i..i + macro_end + "</ac:structured-macro>".len()];
                        let lang = extract_macro_param(macro_str, "language").unwrap_or("");
                        let code = extract_cdata_or_body(macro_str).unwrap_or("");
                        let code_clean = decode_html_entities(code.trim_matches('\n'));
                        let code_md = format!("\n\n```{}\n{}\n```\n\n", lang, code_clean);
                        let idx = blocks.len();
                        blocks.push(code_md);
                        out.push_str(&format!("\n\n___CONFLUENCE_CODE_BLOCK_{}___\n\n", idx));
                        i += macro_end + "</ac:structured-macro>".len();
                        continue;
                    }
                }
            }
        }

        // 2. Standard HTML <pre><code>...</code></pre> or <pre>...</pre>
        if s_lower[i..].starts_with("<pre") {
            let next_byte = html.as_bytes().get(i + 4);
            if next_byte == Some(&b'>') || next_byte == Some(&b' ') {
                if let Some(pre_end) = s_lower[i..].find("</pre>") {
                    let pre_str = &html[i..i + pre_end + "</pre>".len()];
                    if let Some(open_tag_end) = pre_str.find('>') {
                        let content = &pre_str[open_tag_end + 1..pre_str.len() - "</pre>".len()];
                        let code_clean = strip_tags(content);
                        let code_clean = decode_html_entities(&code_clean);
                        let code_md = format!("\n\n```\n{}\n```\n\n", code_clean.trim_matches('\n'));
                        let idx = blocks.len();
                        blocks.push(code_md);
                        out.push_str(&format!("\n\n___CONFLUENCE_CODE_BLOCK_{}___\n\n", idx));
                        i += pre_end + "</pre>".len();
                        continue;
                    }
                }
            }
        }

        let c = html[i..].chars().next().unwrap();
        out.push(c);
        i += c.len_utf8();
    }

    out
}

fn extract_tables(html: &str, tables: &mut Vec<String>) -> String {
    let mut out = String::with_capacity(html.len());
    let mut i = 0;

    while i < html.len() {
        let s_lower = &html[i..].to_lowercase();
        if s_lower.starts_with("<table") {
            let next_byte = html.as_bytes().get(i + 6);
            if next_byte == Some(&b'>') || next_byte == Some(&b' ') {
                if let Some((_, close_end)) = find_matching_close_tag(&html[i..], "<table", "</table>") {
                    let table_html = &html[i..i + close_end];
                    let table_md = table_html_to_markdown(table_html);
                    let idx = tables.len();
                    tables.push(table_md);
                    out.push_str(&format!("\n\n___CONFLUENCE_TABLE_BLOCK_{}___\n\n", idx));
                    i += close_end;
                    continue;
                }
            }
        }
        let c = html[i..].chars().next().unwrap();
        out.push(c);
        i += c.len_utf8();
    }
    out
}

fn find_matching_close_tag(s: &str, open_prefix: &str, close_tag: &str) -> Option<(usize, usize)> {
    let s_lower = s.to_lowercase();
    let mut depth = 0;
    let mut i = 0;
    while i < s.len() {
        if s_lower[i..].starts_with(open_prefix) {
            let next_byte = s.as_bytes().get(i + open_prefix.len());
            if next_byte == Some(&b'>') || next_byte == Some(&b' ') || next_byte == Some(&b'/') {
                depth += 1;
                i += open_prefix.len();
                continue;
            }
        }
        if s_lower[i..].starts_with(close_tag) {
            depth -= 1;
            if depth == 0 {
                return Some((i, i + close_tag.len()));
            }
            i += close_tag.len();
            continue;
        }
        let c = s[i..].chars().next().unwrap();
        i += c.len_utf8();
    }
    None
}

fn table_html_to_markdown(table_html: &str) -> String {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut i = 0;
    let s_lower = table_html.to_lowercase();

    while i < table_html.len() {
        if s_lower[i..].starts_with("<tr") {
            let next_byte = table_html.as_bytes().get(i + 3);
            if next_byte == Some(&b'>') || next_byte == Some(&b' ') {
                if let Some(tr_open_end) = table_html[i..].find('>') {
                    let content_start = i + tr_open_end + 1;
                    if let Some(tr_close_pos) = s_lower[content_start..].find("</tr>") {
                        let tr_content = &table_html[content_start..content_start + tr_close_pos];
                        let row_cells = extract_cells_from_tr(tr_content);
                        if !row_cells.is_empty() {
                            rows.push(row_cells);
                        }
                        i = content_start + tr_close_pos + "</tr>".len();
                        continue;
                    }
                }
            }
        }
        let c = table_html[i..].chars().next().unwrap();
        i += c.len_utf8();
    }

    if rows.is_empty() {
        return String::new();
    }

    let max_cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    if max_cols == 0 {
        return String::new();
    }

    // 将每一行补齐到 max_cols 列，保证列对齐
    for row in &mut rows {
        while row.len() < max_cols {
            row.push(String::new());
        }
    }

    let mut md = String::new();
    // 表头行 (第一行)
    md.push_str("| ");
    md.push_str(&rows[0].join(" | "));
    md.push_str(" |\n");

    // 分隔线
    md.push_str("| ");
    let sep = vec!["---"; max_cols];
    md.push_str(&sep.join(" | "));
    md.push_str(" |\n");

    // 数据行
    for row in &rows[1..] {
        md.push_str("| ");
        md.push_str(&row.join(" | "));
        md.push_str(" |\n");
    }

    md
}

fn extract_cells_from_tr(tr_content: &str) -> Vec<String> {
    let mut cells = Vec::new();
    let mut i = 0;
    let s_lower = tr_content.to_lowercase();

    while i < tr_content.len() {
        let is_th = s_lower[i..].starts_with("<th")
            && (tr_content.as_bytes().get(i + 3) == Some(&b'>')
                || tr_content.as_bytes().get(i + 3) == Some(&b' '));
        let is_td = s_lower[i..].starts_with("<td")
            && (tr_content.as_bytes().get(i + 3) == Some(&b'>')
                || tr_content.as_bytes().get(i + 3) == Some(&b' '));

        if is_th || is_td {
            let close_tag = if is_th { "</th>" } else { "</td>" };
            if let Some(open_tag_end) = tr_content[i..].find('>') {
                let open_tag = &tr_content[i..i + open_tag_end + 1];
                let colspan = extract_tag_attr(open_tag, "colspan")
                    .and_then(|val| val.parse::<usize>().ok())
                    .unwrap_or(1);

                let content_start = i + open_tag_end + 1;
                if let Some(close_pos) = s_lower[content_start..].find(close_tag) {
                    let cell_raw = &tr_content[content_start..content_start + close_pos];
                    let cell_clean = clean_table_cell(cell_raw);
                    cells.push(cell_clean);
                    for _ in 1..colspan {
                        cells.push(String::new());
                    }
                    i = content_start + close_pos + close_tag.len();
                    continue;
                }
            }
        }
        let c = tr_content[i..].chars().next().unwrap();
        i += c.len_utf8();
    }

    cells
}

fn clean_table_cell(html: &str) -> String {
    let mut s = html.to_string();
    s = s.replace("<br/>", "<br>").replace("<br />", "<br>");
    s = s.replace("</p>", "<br>").replace("</div>", "<br>");

    // 行内修饰
    s = s.replace("<b>", "**").replace("</b>", "**")
        .replace("<strong>", "**").replace("</strong>", "**")
        .replace("<code>", "`").replace("</code>", "`")
        .replace("<em>", "*").replace("</em>", "*")
        .replace("<i>", "*").replace("</i>", "*")
        .replace("<del>", "~~").replace("</del>", "~~");

    s = convert_links(&s);
    // 保护 <br> 不被 strip_tags 抹掉
    s = s.replace("<br>", "___BR_PLACEHOLDER___");
    s = strip_tags(&s);
    s = s.replace("___BR_PLACEHOLDER___", "<br>");
    s = decode_html_entities(&s);

    // 转义 Markdown 表格管道符
    s = s.replace('|', "\\|");

    // 将单元格内部换行符替换为空格或保持 <br>，防止表格拆行
    s = s.replace("\r\n", " ").replace('\n', " ").replace('\r', " ");

    while s.contains("<br><br>") {
        s = s.replace("<br><br>", "<br>");
    }
    s = s.trim().to_string();
    while s.starts_with("<br>") {
        s = s[4..].trim_start().to_string();
    }
    while s.ends_with("<br>") {
        s = s[..s.len() - 4].trim_end().to_string();
    }
    s.trim().to_string()
}

fn convert_headings(html: &str) -> String {
    let mut s = html.to_string();
    for level in 1..=6 {
        let open = format!("<h{}", level);
        let close = format!("</h{}>", level);
        let prefix = format!("\n\n{} ", "#".repeat(level));
        let mut i = 0;
        let mut out = String::with_capacity(s.len());
        while i < s.len() {
            let s_lower = &s[i..].to_lowercase();
            if s_lower.starts_with(&open) {
                let next_byte = s.as_bytes().get(i + open.len());
                if next_byte == Some(&b'>') || next_byte == Some(&b' ') {
                    if let Some(tag_end) = s[i..].find('>') {
                        let content_start = i + tag_end + 1;
                        if let Some(close_pos) = s[content_start..].to_lowercase().find(&close) {
                            let heading_text = &s[content_start..content_start + close_pos];
                            let heading_clean = strip_tags(heading_text);
                            let heading_clean = decode_html_entities(&heading_clean);
                            out.push_str(&prefix);
                            out.push_str(heading_clean.trim());
                            out.push_str("\n\n");
                            i = content_start + close_pos + close.len();
                            continue;
                        }
                    }
                }
            }
            let c = s[i..].chars().next().unwrap();
            out.push(c);
            i += c.len_utf8();
        }
        s = out;
    }
    s
}

fn convert_lists(html: &str) -> String {
    let mut s = html.to_string();
    s = process_list_type(&s, "<ol", "</ol>", true);
    s = process_list_type(&s, "<ul", "</ul>", false);
    s
}

fn process_list_type(html: &str, open_tag: &str, close_tag: &str, is_ordered: bool) -> String {
    let mut out = String::with_capacity(html.len());
    let mut i = 0;
    let s_lower = html.to_lowercase();

    while i < html.len() {
        if s_lower[i..].starts_with(open_tag) {
            let next_byte = html.as_bytes().get(i + open_tag.len());
            if next_byte == Some(&b'>') || next_byte == Some(&b' ') {
                if let Some(tag_end) = html[i..].find('>') {
                    let content_start = i + tag_end + 1;
                    if let Some(close_pos) = s_lower[content_start..].find(close_tag) {
                        let list_content = &html[content_start..content_start + close_pos];
                        out.push_str("\n\n");
                        let mut item_idx = 1;
                        let mut item_start = 0;
                        let list_lower = list_content.to_lowercase();
                        while item_start < list_content.len() {
                            if list_lower[item_start..].starts_with("<li") {
                                let next_b = list_content.as_bytes().get(item_start + 3);
                                if next_b == Some(&b'>') || next_b == Some(&b' ') {
                                    if let Some(li_end) = list_content[item_start..].find('>') {
                                        let text_start = item_start + li_end + 1;
                                        if let Some(li_close) = list_lower[text_start..].find("</li>") {
                                            let raw_item = &list_content[text_start..text_start + li_close];
                                            let clean_item = strip_tags(raw_item);
                                            let clean_item = decode_html_entities(&clean_item);
                                            let clean_item = clean_item.trim();
                                            if !clean_item.is_empty() {
                                                if is_ordered {
                                                    out.push_str(&format!("{}. {}\n", item_idx, clean_item));
                                                    item_idx += 1;
                                                } else {
                                                    out.push_str(&format!("- {}\n", clean_item));
                                                }
                                            }
                                            item_start = text_start + li_close + "</li>".len();
                                            continue;
                                        }
                                    }
                                }
                            }
                            let c = list_content[item_start..].chars().next().unwrap();
                            item_start += c.len_utf8();
                        }
                        out.push('\n');
                        i = content_start + close_pos + close_tag.len();
                        continue;
                    }
                }
            }
        }
        let c = html[i..].chars().next().unwrap();
        out.push(c);
        i += c.len_utf8();
    }

    out
}

fn convert_blockquotes(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut i = 0;
    let s_lower = html.to_lowercase();

    while i < html.len() {
        if s_lower[i..].starts_with("<blockquote") {
            let next_byte = html.as_bytes().get(i + 11);
            if next_byte == Some(&b'>') || next_byte == Some(&b' ') {
                if let Some(tag_end) = html[i..].find('>') {
                    let content_start = i + tag_end + 1;
                    if let Some(close_pos) = s_lower[content_start..].find("</blockquote>") {
                        let content = &html[content_start..content_start + close_pos];
                        let clean = strip_tags(content);
                        let clean = decode_html_entities(&clean);
                        out.push_str("\n\n");
                        for line in clean.lines() {
                            let line = line.trim();
                            if !line.is_empty() {
                                out.push_str("> ");
                                out.push_str(line);
                                out.push('\n');
                            }
                        }
                        out.push('\n');
                        i = content_start + close_pos + "</blockquote>".len();
                        continue;
                    }
                }
            }
        }
        let c = html[i..].chars().next().unwrap();
        out.push(c);
        i += c.len_utf8();
    }
    out
}

fn convert_links(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut i = 0;
    let s_lower = html.to_lowercase();

    while i < html.len() {
        if s_lower[i..].starts_with("<a ") || s_lower[i..].starts_with("<a>") {
            if let Some(tag_end) = html[i..].find('>') {
                let tag_str = &html[i..i + tag_end + 1];
                let href = extract_tag_attr(tag_str, "href");
                let content_start = i + tag_end + 1;
                if let Some(close_pos) = s_lower[content_start..].find("</a>") {
                    let link_text = &html[content_start..content_start + close_pos];
                    if let Some(url) = href {
                        let text_clean = strip_tags(link_text);
                        let text_clean = decode_html_entities(&text_clean);
                        let text_clean = text_clean.trim();
                        if text_clean.is_empty() || text_clean == url {
                            out.push_str(url);
                        } else {
                            out.push_str(&format!("[{}]({})", text_clean, url));
                        }
                    } else {
                        out.push_str(link_text);
                    }
                    i = content_start + close_pos + "</a>".len();
                    continue;
                }
            }
        }
        let c = html[i..].chars().next().unwrap();
        out.push(c);
        i += c.len_utf8();
    }
    out
}

fn strip_tags(s: &str) -> String {
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
    out
}

fn decode_html_entities(s: &str) -> String {
    let mut out = s.to_string();
    let entities = [
        ("&nbsp;", " "),
        ("&amp;", "&"),
        ("&lt;", "<"),
        ("&gt;", ">"),
        ("&quot;", "\""),
        ("&#39;", "'"),
        ("&apos;", "'"),
        ("&mdash;", "—"),
        ("&ndash;", "–"),
        ("&hellip;", "..."),
    ];
    for (k, v) in entities {
        out = out.replace(k, v);
    }
    decode_numeric_entities(&out)
}

fn decode_numeric_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i..].starts_with("&#") {
            if let Some(semi) = s[i..].find(';') {
                let ent = &s[i + 2..i + semi];
                let ch = if ent.starts_with('x') || ent.starts_with('X') {
                    u32::from_str_radix(&ent[1..], 16).ok().and_then(char::from_u32)
                } else {
                    ent.parse::<u32>().ok().and_then(char::from_u32)
                };
                if let Some(c) = ch {
                    out.push(c);
                    i += semi + 1;
                    continue;
                }
            }
        }
        let c = s[i..].chars().next().unwrap();
        out.push(c);
        i += c.len_utf8();
    }
    out
}

fn normalize_markdown(s: &str) -> String {
    let lines: Vec<&str> = s.lines().map(str::trim_end).collect();
    let mut result = Vec::new();
    let mut empty_count = 0;
    for line in lines {
        if line.trim().is_empty() {
            empty_count += 1;
            if empty_count <= 1 && !result.is_empty() {
                result.push("");
            }
        } else {
            empty_count = 0;
            result.push(line);
        }
    }
    result.join("\n").trim().to_string()
}

struct ParsedTable {
    table_start: usize,
    table_end: usize,
    rows: Vec<ParsedRow>,
}

struct ParsedRow {
    row_start: usize,
    row_end: usize,
    cells: Vec<ParsedCell>,
    plain_text: String,
}

struct ParsedCell {
    is_th: bool,
    is_empty_tag: bool,
    tag_start: usize,
    inner_start: usize,
    inner_end: usize,
    tag_end: usize,
    text: String,
    #[allow(dead_code)]
    colspan: usize,
}

fn parse_tables_with_xml(xml: &str) -> Vec<ParsedTable> {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    reader.config_mut().check_end_names = false;

    let mut tables = Vec::new();
    let mut current_table: Option<ParsedTable> = None;
    let mut current_row: Option<ParsedRow> = None;
    let mut current_cell: Option<ParsedCell> = None;
    let mut table_depth = 0;
    let mut cell_depth = 0;

    loop {
        let before = reader.buffer_position() as usize;
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let after = reader.buffer_position() as usize;
                let name = e.local_name();
                if name.as_ref() == b"table" {
                    table_depth += 1;
                    if table_depth == 1 {
                        current_table = Some(ParsedTable {
                            table_start: before,
                            table_end: 0,
                            rows: Vec::new(),
                        });
                    }
                } else if name.as_ref() == b"tr" && table_depth == 1 {
                    current_row = Some(ParsedRow {
                        row_start: before,
                        row_end: 0,
                        cells: Vec::new(),
                        plain_text: String::new(),
                    });
                } else if (name.as_ref() == b"th" || name.as_ref() == b"td")
                    && table_depth == 1
                    && current_row.is_some()
                {
                    if cell_depth == 0 {
                        let is_th = name.as_ref() == b"th";
                        let tag_str = String::from_utf8_lossy(&xml.as_bytes()[before..after]);
                        let colspan = extract_tag_attr(&tag_str, "colspan")
                            .and_then(|v| v.parse::<usize>().ok())
                            .unwrap_or(1);
                        current_cell = Some(ParsedCell {
                            is_th,
                            is_empty_tag: false,
                            tag_start: before,
                            inner_start: after,
                            inner_end: 0,
                            tag_end: 0,
                            text: String::new(),
                            colspan,
                        });
                    }
                    cell_depth += 1;
                }
            }
            Ok(Event::Empty(ref e)) => {
                let after = reader.buffer_position() as usize;
                let name = e.local_name();
                if (name.as_ref() == b"th" || name.as_ref() == b"td")
                    && table_depth == 1
                    && current_row.is_some()
                    && cell_depth == 0
                {
                    let is_th = name.as_ref() == b"th";
                    let tag_str = String::from_utf8_lossy(&xml.as_bytes()[before..after]);
                    let colspan = extract_tag_attr(&tag_str, "colspan")
                        .and_then(|v| v.parse::<usize>().ok())
                        .unwrap_or(1);
                    if let Some(ref mut row) = current_row {
                        row.cells.push(ParsedCell {
                            is_th,
                            is_empty_tag: true,
                            tag_start: before,
                            inner_start: before,
                            inner_end: after,
                            tag_end: after,
                            text: String::new(),
                            colspan,
                        });
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                let after = reader.buffer_position() as usize;
                let name = e.local_name();
                if (name.as_ref() == b"th" || name.as_ref() == b"td") && cell_depth > 0 {
                    cell_depth -= 1;
                    if cell_depth == 0 {
                        if let Some(mut cell) = current_cell.take() {
                            cell.inner_end = before;
                            cell.tag_end = after;
                            let inner_raw = &xml[cell.inner_start..cell.inner_end];
                            cell.text = strip_tags(inner_raw).trim().to_string();
                            if let Some(ref mut row) = current_row {
                                row.plain_text.push(' ');
                                row.plain_text.push_str(&cell.text);
                                row.cells.push(cell);
                            }
                        }
                    }
                } else if name.as_ref() == b"tr" {
                    if let Some(mut row) = current_row.take() {
                        row.row_end = after;
                        if let Some(ref mut table) = current_table {
                            table.rows.push(row);
                        }
                    }
                } else if name.as_ref() == b"table" {
                    table_depth -= 1;
                    if table_depth == 0 {
                        if let Some(mut table) = current_table.take() {
                            table.table_end = after;
                            tables.push(table);
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }

    tables
}

pub(crate) fn table_set_cell(
    orig_html: &str,
    row_anchor: &str,
    col_spec: &str,
    new_value: &str,
) -> Result<(String, Value), AppError> {
    let tables = parse_tables_with_xml(orig_html);
    if tables.is_empty() {
        return Err(AppError::param_invalid("原页面中未找到任何 HTML 表格"));
    }

    // 寻找包含 row_anchor 的所有行 (跨所有表格)
    let anchor_trimmed = row_anchor.trim();
    let mut matches = Vec::new();

    for (t_idx, table) in tables.iter().enumerate() {
        for (r_idx, row) in table.rows.iter().enumerate() {
            let row_slice = &orig_html[row.row_start..row.row_end];
            if row.plain_text.contains(anchor_trimmed) || row_slice.contains(anchor_trimmed) {
                matches.push((t_idx, r_idx));
            }
        }
    }

    if matches.is_empty() {
        return Err(AppError::param_invalid(format!(
            "❌ 错误: 未在任何表格中匹配到包含行锚点 '{}' 的行。请使用 confluence get 确认页面表格内容",
            row_anchor
        )));
    }

    if matches.len() > 1 {
        return Err(AppError::param_invalid(format!(
            "❌ 错误: 行锚点 '{}' 匹配到了 {} 个不同的表格行 (存在二义性)。请提供该行更具唯一性的特征文本",
            row_anchor,
            matches.len()
        )));
    }

    let (t_idx, r_idx) = matches[0];
    let table = &tables[t_idx];
    let row = &table.rows[r_idx];

    // 获取表头名称清单 (优先从首行或首个 th 行读取)
    let header_row = table
        .rows
        .iter()
        .find(|r| r.cells.iter().any(|c| c.is_th))
        .unwrap_or(&table.rows[0]);
    let header_names: Vec<String> = header_row
        .cells
        .iter()
        .map(|c| c.text.trim().to_string())
        .collect();

    // 确定目标列号 target_col_idx
    let target_col_idx = if let Ok(num) = col_spec.trim().parse::<usize>() {
        if num == 0 {
            0
        } else {
            num - 1 // 1-based 转 0-based
        }
    } else {
        let col_spec_clean = col_spec.trim().to_lowercase();
        let mut found_idx = None;
        for (idx, h_name) in header_names.iter().enumerate() {
            let h_clean = h_name.to_lowercase();
            if h_clean == col_spec_clean || h_clean.contains(&col_spec_clean) {
                found_idx = Some(idx);
                break;
            }
        }
        match found_idx {
            Some(idx) => idx,
            None => {
                return Err(AppError::param_invalid(format!(
                    "❌ 错误: 在目标表格中未找到列名 '{}'。该表格包含的表头列为: {:?}",
                    col_spec, header_names
                )));
            }
        }
    };

    if target_col_idx >= row.cells.len() {
        return Err(AppError::param_invalid(format!(
            "❌ 错误: 目标行只有 {} 列，无法更新第 {} 列",
            row.cells.len(),
            target_col_idx + 1
        )));
    }

    let target_cell = &row.cells[target_col_idx];
    let old_cell_html = if target_cell.is_empty_tag {
        String::new()
    } else {
        orig_html[target_cell.inner_start..target_cell.inner_end].to_string()
    };
    let old_cell_text = target_cell.text.clone();

    // 格式化新单元格内容 (宏展开与安全转义)
    let formatted_value = format_cell_value(new_value);

    // 物理切片拼接
    let new_html = if target_cell.is_empty_tag {
        let tag_name = if target_cell.is_th { "th" } else { "td" };
        format!(
            "{}<{}>{}</{}>{}",
            &orig_html[..target_cell.tag_start],
            tag_name,
            formatted_value,
            tag_name,
            &orig_html[target_cell.tag_end..]
        )
    } else {
        format!(
            "{}{}{}",
            &orig_html[..target_cell.inner_start],
            formatted_value,
            &orig_html[target_cell.inner_end..]
        )
    };

    let col_display_name = header_names
        .get(target_col_idx)
        .cloned()
        .unwrap_or_else(|| format!("Col {}", target_col_idx + 1));

    let diff = json!({
        "mode": "set_cell",
        "row_anchor": row_anchor,
        "column": col_display_name,
        "col_index": target_col_idx + 1,
        "old_value": old_cell_text,
        "new_value": new_value,
        "old_html": old_cell_html,
        "new_html": formatted_value,
    });

    Ok((new_html, diff))
}

pub(crate) fn table_append_row(
    orig_html: &str,
    table_anchor: &str,
    cells_values: &[String],
) -> Result<(String, Value), AppError> {
    let tables = parse_tables_with_xml(orig_html);
    if tables.is_empty() {
        return Err(AppError::param_invalid("原页面中未找到任何 HTML 表格"));
    }

    let anchor_trimmed = table_anchor.trim();
    let target_table_idx = if tables.len() == 1 && anchor_trimmed.is_empty() {
        0
    } else {
        let mut matches = Vec::new();
        for (idx, table) in tables.iter().enumerate() {
            let table_slice = &orig_html[table.table_start..table.table_end];
            if table_slice.contains(anchor_trimmed) {
                matches.push(idx);
            }
        }
        if matches.is_empty() {
            return Err(AppError::param_invalid(format!(
                "❌ 错误: 未在页面中找到包含锚点 '{}' 的表格",
                table_anchor
            )));
        }
        if matches.len() > 1 {
            return Err(AppError::param_invalid(format!(
                "❌ 错误: 锚点 '{}' 匹配到了 {} 个表格 (存在二义性)。请提供更精确的表格特征文本",
                table_anchor,
                matches.len()
            )));
        }
        matches[0]
    };

    let table = &tables[target_table_idx];
    if table.rows.is_empty() {
        return Err(AppError::param_invalid("目标表格不包含任何行，无法追加新行"));
    }

    let last_row = table.rows.last().unwrap();
    let insert_pos = last_row.row_end;

    // 构造新 <tr>
    let mut new_tr = String::from("<tr>");
    for val in cells_values {
        let formatted = format_cell_value(val);
        new_tr.push_str(&format!("<td>{}</td>", formatted));
    }
    new_tr.push_str("</tr>");

    let new_html = format!(
        "{}\n  {}\n{}",
        &orig_html[..insert_pos],
        new_tr,
        &orig_html[insert_pos..]
    );

    let diff = json!({
        "mode": "append_row",
        "table_anchor": table_anchor,
        "appended_cells": cells_values,
        "new_row_html": new_tr,
    });

    Ok((new_html, diff))
}

fn status_color_for_title(title: &str) -> &'static str {
    let t = title.trim().to_uppercase();
    match t.as_str() {
        "DONE" | "SUCCESS" | "RESOLVED" | "FINISHED" | "CLOSED" | "已完成" | "完成" | "通过" => {
            "Green"
        }
        "IN PROGRESS" | "IN_PROGRESS" | "DOING" | "RUNNING" | "进行中" | "开发中" => "Blue",
        "TODO" | "OPEN" | "PENDING" | "BACKLOG" | "待办" | "未开始" => "Grey",
        "FAILED" | "ERROR" | "BLOCKED" | "REJECTED" | "阻塞" | "失败" | "已废弃" => "Red",
        "REVIEW" | "WARNING" | "HOLD" | "PAUSED" | "评审中" | "挂起" | "待确认" => "Yellow",
        _ => "Blue",
    }
}

fn format_cell_value(val: &str) -> String {
    let trimmed = val.trim();
    if trimmed.starts_with('<') {
        return val.to_string();
    }
    format_macro_shorthands(trimmed)
}

fn format_macro_shorthands(text: &str) -> String {
    let mut s = text.to_string();

    // 1. [Status: TITLE] 或 [Status: TITLE | COLOR]
    while let Some(pos) = s.find("[Status:") {
        if let Some(end) = s[pos..].find(']') {
            let full_match = &s[pos..pos + end + 1];
            let inner = &s[pos + "[Status:".len()..pos + end];
            let (title, color) = if let Some(pipe) = inner.find('|') {
                (inner[..pipe].trim(), inner[pipe + 1..].trim().to_string())
            } else {
                let t = inner.trim();
                (t, status_color_for_title(t).to_string())
            };
            let macro_xml = format!(
                r#"<ac:structured-macro ac:name="status"><ac:parameter ac:name="colour">{}</ac:parameter><ac:parameter ac:name="title">{}</ac:parameter></ac:structured-macro>"#,
                color, title
            );
            s = s.replace(full_match, &macro_xml);
        } else {
            break;
        }
    }

    // 2. [~username]
    while let Some(pos) = s.find("[~") {
        if let Some(end) = s[pos..].find(']') {
            let full_match = &s[pos..pos + end + 1];
            let uname = s[pos + 2..pos + end].trim();
            if !uname.is_empty() && !uname.contains(' ') {
                let macro_xml = format!(r#"<ac:link><ri:user ri:username="{}"/></ac:link>"#, uname);
                s = s.replace(full_match, &macro_xml);
                continue;
            }
        }
        break;
    }

    // 3. [Date: YYYY-MM-DD]
    while let Some(pos) = s.find("[Date:") {
        if let Some(end) = s[pos..].find(']') {
            let full_match = &s[pos..pos + end + 1];
            let dt = s[pos + "[Date:".len()..pos + end].trim();
            let macro_xml = format!(r#"<time datetime="{}"/>"#, dt);
            s = s.replace(full_match, &macro_xml);
        } else {
            break;
        }
    }

    // 4. XML 实体转义
    escape_xml_text(&s)
}

fn escape_xml_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i..].starts_with('<') {
            if let Some(tag_end) = s[i..].find('>') {
                let tag_candidate = &s[i + 1..i + tag_end];
                let is_tag = tag_candidate.starts_with('/')
                    || tag_candidate.starts_with("ac:")
                    || tag_candidate.starts_with("ri:")
                    || tag_candidate.starts_with("time")
                    || tag_candidate.starts_with("p")
                    || tag_candidate.starts_with("b")
                    || tag_candidate.starts_with("strong")
                    || tag_candidate.starts_with("em")
                    || tag_candidate.starts_with("i")
                    || tag_candidate.starts_with("code")
                    || tag_candidate.starts_with("a")
                    || tag_candidate.starts_with("span")
                    || tag_candidate.starts_with("div")
                    || tag_candidate.starts_with("table")
                    || tag_candidate.starts_with("tr")
                    || tag_candidate.starts_with("th")
                    || tag_candidate.starts_with("td");
                if is_tag {
                    out.push_str(&s[i..i + tag_end + 1]);
                    i += tag_end + 1;
                    continue;
                }
            }
        }
        let c = s[i..].chars().next().unwrap();
        match c {
            '&' if !s[i..].starts_with("&amp;")
                && !s[i..].starts_with("&lt;")
                && !s[i..].starts_with("&gt;")
                && !s[i..].starts_with("&quot;")
                && !s[i..].starts_with("&apos;") =>
            {
                out.push_str("&amp;");
            }
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
        i += c.len_utf8();
    }
    out
}

/// 自动将纯文本/Markdown/HTML 转为 Confluence Storage 格式 (带宏简写与实体保护)
fn format_to_storage_html(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.starts_with('<') || trimmed.contains("<p>") || trimmed.contains("<ac:") || trimmed.contains("<time") {
        text.to_string()
    } else {
        let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        lines
            .iter()
            .map(|l| format!("<p>{}</p>", format_macro_shorthands(l)))
            .collect::<Vec<_>>()
            .join("\n")
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

    #[test]
    fn test_table_preserves_columns_and_empty_cells() {
        let html = r#"
        <table class="wrapped">
            <tbody>
                <tr>
                    <th>模块</th>
                    <th>责任人</th>
                    <th>状态</th>
                    <th>上线日期</th>
                </tr>
                <tr>
                    <td>用户中心</td>
                    <td><ri:user ri:username="zhangsan"/></td>
                    <td><ac:structured-macro ac:name="status"><ac:parameter ac:name="title">DONE</ac:parameter></ac:structured-macro></td>
                    <td><time datetime="2026-08-15"/></td>
                </tr>
                <tr>
                    <td>支付网关</td>
                    <td><ri:user ri:username="lisi"/></td>
                    <td><ac:structured-macro ac:name="status"><ac:parameter ac:name="title">IN PROGRESS</ac:parameter></ac:structured-macro></td>
                    <td></td>
                </tr>
            </tbody>
        </table>
        "#;

        let md = html_to_markdown(html);
        assert!(md.contains("| 模块 | 责任人 | 状态 | 上线日期 |"));
        assert!(md.contains("| --- | --- | --- | --- |"));
        assert!(md.contains("| 用户中心 | [~zhangsan] | [Status: DONE] | [Date: 2026-08-15] |"));
        // 关键点：空单元格保留为 |  |，列不会向前塌陷！
        assert!(md.contains("| 支付网关 | [~lisi] | [Status: IN PROGRESS] |  |"));
    }

    #[test]
    fn test_table_with_multiline_cells_and_pipes() {
        let html = r#"
        <table>
            <tr>
                <th>接口</th>
                <th>方法 | 路径</th>
                <th>说明</th>
            </tr>
            <tr>
                <td>登录</td>
                <td>POST /login | /auth</td>
                <td><p>第一行说明</p><p>第二行说明</p></td>
            </tr>
        </table>
        "#;

        let md = html_to_markdown(html);
        assert!(md.contains(r#"POST /login \| /auth"#));
        assert!(md.contains("第一行说明<br>第二行说明"));
    }

    #[test]
    fn test_table_with_colspan() {
        let html = r#"
        <table>
            <tr>
                <th colspan="2">全宽标题</th>
                <th>其它</th>
            </tr>
            <tr>
                <td>列1</td>
                <td>列2</td>
                <td>列3</td>
            </tr>
        </table>
        "#;

        let md = html_to_markdown(html);
        assert!(md.contains("| 全宽标题 |  | 其它 |"));
        assert!(md.contains("| 列1 | 列2 | 列3 |"));
    }

    #[test]
    fn test_code_block_macro() {
        let html = r#"
        <p>示例代码：</p>
        <ac:structured-macro ac:name="code">
            <ac:parameter ac:name="language">rust</ac:parameter>
            <ac:plain-text-body><![CDATA[fn main() {
    println!("Hello, Confluence!");
}]]></ac:plain-text-body>
        </ac:structured-macro>
        "#;

        let md = html_to_markdown(html);
        assert!(md.contains("```rust"));
        assert!(md.contains("println!(\"Hello, Confluence!\");"));
    }

    #[test]
    fn test_table_set_cell_by_column_name_and_macro() {
        let html = r#"<p>前置段落</p>
<table>
  <tbody>
    <tr><th>模块</th><th>责任人</th><th>状态</th></tr>
    <tr><td>用户中心</td><td>[~zhangsan]</td><td><ac:structured-macro ac:name="status"><ac:parameter ac:name="title">IN PROGRESS</ac:parameter></ac:structured-macro></td></tr>
    <tr><td>支付网关</td><td>[~lisi]</td><td><ac:structured-macro ac:name="status"><ac:parameter ac:name="title">TODO</ac:parameter></ac:structured-macro></td></tr>
  </tbody>
</table>
<p>后置段落</p>"#;

        let (new_html, diff) = table_set_cell(html, "支付网关", "状态", "[Status: DONE]").unwrap();
        assert!(new_html.contains(r#"ac:name="status"><ac:parameter ac:name="colour">Green</ac:parameter><ac:parameter ac:name="title">DONE</ac:parameter>"#));
        // 用户中心的状态保持不变
        assert!(new_html.contains("IN PROGRESS"));
        // 前后段落完好无损
        assert!(new_html.contains("<p>前置段落</p>"));
        assert!(new_html.contains("<p>后置段落</p>"));
        assert_eq!(diff["mode"], "set_cell");
        assert_eq!(diff["column"], "状态");
        assert_eq!(diff["col_index"], 3);
    }

    #[test]
    fn test_table_set_cell_by_column_index() {
        let html = r#"<table>
  <tr><th>Key</th><th>Val</th></tr>
  <tr><td>HOST</td><td>127.0.0.1</td></tr>
</table>"#;

        let (new_html, diff) = table_set_cell(html, "HOST", "2", "192.168.1.1").unwrap();
        assert!(new_html.contains("<td>192.168.1.1</td>"));
        assert!(!new_html.contains("127.0.0.1"));
        assert_eq!(diff["col_index"], 2);
    }

    #[test]
    fn test_table_set_cell_ambiguity_and_missing_defense() {
        let html = r#"<table>
  <tr><th>A</th><th>B</th></tr>
  <tr><td>DUP</td><td>1</td></tr>
  <tr><td>DUP</td><td>2</td></tr>
</table>"#;

        // 1. 未找到行
        let err1 = table_set_cell(html, "NON_EXIST", "B", "val").unwrap_err();
        assert!(err1.to_string().contains("未在任何表格中匹配到包含行锚点"));

        // 2. 二义性多行匹配
        let err2 = table_set_cell(html, "DUP", "B", "val").unwrap_err();
        assert!(err2.to_string().contains("存在二义性"));

        // 3. 列名不存在 (列出可用列)
        let html_single = r#"<table><tr><th>模块</th><th>状态</th></tr><tr><td>单行</td><td>OK</td></tr></table>"#;
        let err3 = table_set_cell(html_single, "单行", "不存在的列", "val").unwrap_err();
        assert!(err3.to_string().contains("该表格包含的表头列为"));
    }

    #[test]
    fn test_table_append_row() {
        let html = r#"<table class="wrapped">
  <tbody>
    <tr><th>模块</th><th>状态</th></tr>
    <tr><td>旧模块</td><td>DONE</td></tr>
  </tbody>
</table>"#;

        let cells = vec!["新模块".to_string(), "[Status: IN PROGRESS]".to_string()];
        let (new_html, diff) = table_append_row(html, "旧模块", &cells).unwrap();
        assert!(new_html.contains("<td>新模块</td>"));
        assert!(new_html.contains(r#"ac:name="status"><ac:parameter ac:name="colour">Blue</ac:parameter><ac:parameter ac:name="title">IN PROGRESS</ac:parameter>"#));
        assert_eq!(diff["mode"], "append_row");
    }

    #[test]
    fn test_format_macro_shorthands_and_xml_escape() {
        // 宏简写展开
        let s1 = format_macro_shorthands("责任人: [~zhangsan], 状态: [Status: DONE], 日期: [Date: 2026-08-20]");
        assert!(s1.contains(r#"<ac:link><ri:user ri:username="zhangsan"/></ac:link>"#));
        assert!(s1.contains(r#"colour">Green</ac:parameter><ac:parameter ac:name="title">DONE"#));
        assert!(s1.contains(r#"<time datetime="2026-08-20"/>"#));

        // XML 实体防爆保护
        let s2 = format_macro_shorthands("A & B (val < 100)");
        assert_eq!(s2, "A &amp; B (val &lt; 100)");
    }

    #[test]
    fn test_user_mention_with_userkey_and_link_body() {
        // Bug 2 验证: ri:userkey + ac:plain-text-link-body 必须还原出人名，绝不为空
        let html1 = r#"<p>Hi <ac:link><ri:user ri:userkey="402880824ff000000000000000000001"/><ac:plain-text-link-body><![CDATA[zhang san]]></ac:plain-text-link-body></ac:link>, please review.</p>"#;
        let md1 = html_to_markdown(html1);
        assert!(md1.contains("Hi [~zhang san], please review."));

        // 仅有 ri:userkey 无显示名时保底输出 userkey 标识，绝不变成两个连续空格
        let html2 = r#"<p>Hi <ri:user ri:userkey="402880824ff000000000000000000001"/>, please check.</p>"#;
        let md2 = html_to_markdown(html2);
        assert!(md2.contains("Hi [~user:402880824ff000000000000000000001], please check."));
    }

    #[test]
    fn test_extract_markdown_section() {
        let doc = r#"
# 项目主页

## Overview
这是项目背景与总体概览内容。

## Comments
张三：我同意整体架构。
李四：接口签名需要再审视。

### 遗留问题
1. 缺少限流策略。

## 附录
其他资料。
"#;

        let sec = extract_markdown_section(doc, "Comments").unwrap();
        assert!(sec.starts_with("## Comments"));
        assert!(sec.contains("张三：我同意整体架构。"));
        assert!(sec.contains("### 遗留问题"));
        assert!(sec.contains("1. 缺少限流策略。"));
        // 不包含下一个同级标题 "## 附录"
        assert!(!sec.contains("## 附录"));
    }

    #[test]
    fn test_extract_markdown_table() {
        let doc = r#"
## 正文
说明文字。

| 表1_列A | 表1_列B |
| --- | --- |
| 11 | 12 |

中间段落。

| 模块 | 责任人 | 状态 |
| --- | --- | --- |
| 网关 | [~lisi] | DONE |
"#;

        // 1. 按序号提取第 2 张表
        let t2 = extract_markdown_table(doc, "2").unwrap();
        assert!(t2.contains("| 模块 | 责任人 | 状态 |"));
        assert!(!t2.contains("表1_列A"));

        // 2. 按表头关键字提取
        let t_gw = extract_markdown_table(doc, "模块").unwrap();
        assert!(t_gw.contains("| 网关 | [~lisi] | DONE |"));
    }
}
