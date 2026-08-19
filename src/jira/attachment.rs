use serde_json::{json, Value};

use super::Jira;
use crate::error::AppError;
use crate::utils::parse_jira_key;

impl Jira {
    /// GET /rest/api/2/issue/{key}?fields=attachment (查询工单挂载的全部附件列表)
    pub async fn list_attachments(&self, key_or_url: &str) -> Result<Value, AppError> {
        let key = parse_jira_key(key_or_url);
        let path = format!("/rest/api/2/issue/{}", urlencoding::encode(&key));

        let raw = self
            .http
            .get_with_query(&path, &[("fields", "attachment")])
            .await?;

        let attachments = raw["fields"]["attachment"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .map(|att| {
                        json!({
                            "id": att["id"].as_str().unwrap_or(""),
                            "filename": att["filename"].as_str().unwrap_or(""),
                            "size": att["size"].as_u64().unwrap_or(0),
                            "mime_type": att["mimeType"].as_str().unwrap_or(""),
                            "created": att["created"].as_str().unwrap_or(""),
                            "author": att["author"]["displayName"].as_str().or(att["author"]["name"].as_str()).unwrap_or(""),
                            "download_url": att["content"].as_str().unwrap_or(""),
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Ok(json!({
            "issue_key": key,
            "count": attachments.len(),
            "attachments": attachments,
            "url": format!("{}/browse/{}", self.http.base_url(), key),
        }))
    }

    /// POST /rest/api/2/issue/{key}/attachments (上传本地文件到 Jira 工单作为附件)
    pub async fn attach_file(&self, key_or_url: &str, file_path_str: &str) -> Result<Value, AppError> {
        let key = parse_jira_key(key_or_url);
        let path_obj = std::path::Path::new(file_path_str.trim());
        if !path_obj.exists() {
            return Err(AppError::param_invalid(format!("本地文件不存在: {}", file_path_str)));
        }
        let file_name = path_obj
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("attachment")
            .to_string();

        let endpoint = format!("/rest/api/2/issue/{}/attachments", urlencoding::encode(&key));

        if self.policy.dry_run {
            // 预览仅展示文件名与大小,不读取文件内容
            let size = path_obj.metadata().map(|m| m.len()).unwrap_or(0);
            let body = json!({ "files": [format!("{} ({} bytes)", file_name, size)] });
            return Ok(crate::module::preview_json(
                "jira.attach",
                "POST(multipart)",
                &endpoint,
                &key,
                Some(&body),
                None,
            ));
        }
        crate::module::require_confirmed(&self.policy)?;

        let file_bytes = tokio::fs::read(path_obj).await?;
        let part = reqwest::multipart::Part::bytes(file_bytes).file_name(file_name.clone());
        let form = reqwest::multipart::Form::new().part("file", part);

        let raw = self.http.post_multipart(&endpoint, form).await?;

        Ok(json!({
            "status": "success",
            "issue_key": key,
            "filename": file_name,
            "result": raw,
            "url": format!("{}/browse/{}", self.http.base_url(), key),
        }))
    }

    /// GET /rest/api/2/attachment/{id} 或根据 issue 附件列表匹配下载二进制流保存至本地
    pub async fn download_attachment(
        &self,
        key_or_url: &str,
        attachment_id_or_name: &str,
        output_path: Option<&str>,
    ) -> Result<Value, AppError> {
        let key = parse_jira_key(key_or_url);
        let path = format!("/rest/api/2/issue/{}", urlencoding::encode(&key));

        let raw = self
            .http
            .get_with_query(&path, &[("fields", "attachment")])
            .await?;

        let att_list = raw["fields"]["attachment"].as_array().ok_or_else(|| {
            AppError::not_found(format!("工单 {} 上未找到任何附件", key))
        })?;

        let target_str = attachment_id_or_name.trim();

        // 匹配附件 (按 ID 或文件名，忽略大小写)
        let matched = att_list
            .iter()
            .find(|att| {
                let id = att["id"].as_str().unwrap_or("");
                let filename = att["filename"].as_str().unwrap_or("");
                id == target_str || filename.eq_ignore_ascii_case(target_str)
            })
            .ok_or_else(|| {
                AppError::not_found(format!(
                    "工单 {} 上未找到 ID 或文件名为 '{}' 的附件",
                    key, target_str
                ))
            })?;

        let att_id = matched["id"].as_str().unwrap_or("");
        let filename = matched["filename"].as_str().unwrap_or("attachment");
        let download_url = matched["content"].as_str().ok_or_else(|| {
            AppError::generic("附件元数据中缺少 download content URL")
        })?;

        let bytes = self.http.get_bytes(download_url).await?;

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
            "issue_key": key,
            "attachment_id": att_id,
            "filename": filename,
            "size": bytes.len(),
            "saved_path": abs_path,
        }))
    }

    /// DELETE /rest/api/2/issue/{key}/attachments/{id} (删除附件,支持 ID 或文件名)
    pub async fn delete_attachment(
        &self,
        key_or_url: &str,
        attachment_id_or_name: &str,
    ) -> Result<Value, AppError> {
        let key = parse_jira_key(key_or_url);
        let path = format!("/rest/api/2/issue/{}", urlencoding::encode(&key));

        // 解析附件 ID(支持 ID 或文件名,忽略大小写)
        let raw = self
            .http
            .get_with_query(&path, &[("fields", "attachment")])
            .await?;
        let att_list = raw["fields"]["attachment"].as_array().ok_or_else(|| {
            AppError::not_found(format!("工单 {} 上未找到任何附件", key))
        })?;
        let target_str = attachment_id_or_name.trim();
        let matched = att_list
            .iter()
            .find(|att| {
                let id = att["id"].as_str().unwrap_or("");
                let filename = att["filename"].as_str().unwrap_or("");
                id == target_str || filename.eq_ignore_ascii_case(target_str)
            })
            .ok_or_else(|| {
                AppError::not_found(format!(
                    "工单 {} 上未找到 ID 或文件名为 '{}' 的附件",
                    key, target_str
                ))
            })?;
        let att_id = matched["id"].as_str().unwrap_or("");

        let del_path = format!(
            "/rest/api/2/issue/{}/attachments/{}",
            urlencoding::encode(&key),
            urlencoding::encode(att_id)
        );
        if self.policy.dry_run {
            let body = json!({ "attachment_id": att_id, "filename": matched["filename"] });
            return Ok(crate::module::preview_json(
                "jira.attachment-delete", "DELETE", &del_path, &key, Some(&body), None,
            ));
        }
        crate::module::require_confirmed(&self.policy)?;
        let raw = self.http.delete(&del_path).await?;
        if crate::module::is_replayed(&raw) {
            return Ok(raw);
        }
        Ok(json!({
            "status": "success",
            "issue": key,
            "attachment_id": att_id,
            "filename": matched["filename"],
            "deleted": true,
        }))
    }
}
