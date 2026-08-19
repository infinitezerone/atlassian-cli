use serde_json::{json, Value};

use super::Jira;
use crate::error::AppError;
use crate::utils::parse_jira_key;

impl Jira {
    /// POST /rest/api/2/issue/{key}/comment (支持直接传入 Issue Key 或网页 URL)
    pub async fn add_comment(&self, key_or_url: &str, text: &str) -> Result<Value, AppError> {
        // 提及语法校验:拦 [~xxx] 格式错误,防止 AI 拼错 @人
        crate::utils::validate_mentions(text)?;
        let key = parse_jira_key(key_or_url);
        let enc_key = urlencoding::encode(&key);
        let path = format!("/rest/api/2/issue/{}/comment", enc_key);
        let body = json!({ "body": text });
        if self.policy.dry_run {
            return Ok(crate::module::preview_json("jira.comment", "POST", &path, &key, Some(&body), None));
        }
        crate::module::require_confirmed(&self.policy)?;
        let raw = self.http.post(&path, body).await?;
        if crate::module::is_replayed(&raw) {
            return Ok(raw);
        }
        Ok(json!({
            "status": "success",
            "issue": key,
            "comment_id": raw["id"],
            "author": raw["author"]["displayName"],
        }))
    }

    /// PUT /rest/api/2/issue/{key}/comment/{id} (编辑已有评论)
    pub async fn update_comment(
        &self,
        key_or_url: &str,
        comment_id: &str,
        text: &str,
    ) -> Result<Value, AppError> {
        // 提及语法校验:编辑后的内容同样校验
        crate::utils::validate_mentions(text)?;
        let key = parse_jira_key(key_or_url);
        let enc_key = urlencoding::encode(&key);
        let enc_id = urlencoding::encode(comment_id.trim());
        let path = format!("/rest/api/2/issue/{}/comment/{}", enc_key, enc_id);
        let body = json!({ "body": text });
        if self.policy.dry_run {
            return Ok(crate::module::preview_json("jira.comment-update", "PUT", &path, &key, Some(&body), None));
        }
        crate::module::require_confirmed(&self.policy)?;
        let raw = self.http.put(&path, body).await?;
        if crate::module::is_replayed(&raw) {
            return Ok(raw);
        }
        Ok(json!({
            "status": "success",
            "issue": key,
            "comment_id": comment_id.trim(),
            "updated": true,
            "author": raw["author"]["displayName"],
        }))
    }

    /// DELETE /rest/api/2/issue/{key}/comment/{id} (删除评论)
    pub async fn delete_comment(
        &self,
        key_or_url: &str,
        comment_id: &str,
    ) -> Result<Value, AppError> {
        let key = parse_jira_key(key_or_url);
        let enc_key = urlencoding::encode(&key);
        let enc_id = urlencoding::encode(comment_id.trim());
        let path = format!("/rest/api/2/issue/{}/comment/{}", enc_key, enc_id);
        if self.policy.dry_run {
            return Ok(crate::module::preview_json("jira.comment-delete", "DELETE", &path, &key, None, None));
        }
        crate::module::require_confirmed(&self.policy)?;
        let raw = self.http.delete(&path).await?;
        if crate::module::is_replayed(&raw) {
            return Ok(raw);
        }
        Ok(json!({
            "status": "success",
            "issue": key,
            "comment_id": comment_id.trim(),
            "deleted": true,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::HttpClient;
    use crate::module::WritePolicy;

    #[tokio::test]
    async fn test_mock_jira_add_comment() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let key = format!("PROJ-{}", nanos % 1000000);
        let path_str = format!("/rest/api/2/issue/{}/comment", key);

        Mock::given(method("POST"))
            .and(path(path_str))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({
                "id": "9999",
                "body": "Fixed in PR #42",
                "author": { "displayName": "Zhang San" },
                "created": "2026-08-18T12:00:00.000+0000"
            })))
            .mount(&mock_server)
            .await;

        let http = HttpClient::new(mock_server.uri(), "token", false).unwrap();
        let jira = Jira::new(http, WritePolicy { dry_run: false, confirm: true });

        let res = jira.add_comment(&key, "Fixed in PR #42").await.unwrap();
        assert_eq!(res["status"], "success");
        assert_eq!(res["comment_id"], "9999");
        assert_eq!(res["issue"], key);
    }
}
