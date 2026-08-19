use serde_json::{json, Value};

use super::Jira;
use crate::error::AppError;
use crate::utils::parse_jira_key;

impl Jira {
    /// POST /rest/api/2/issue/{key}/transitions (支持直接传入 Issue Key 或网页 URL)
    pub async fn transition(&self, key_or_url: &str, status: &str) -> Result<Value, AppError> {
        let key = parse_jira_key(key_or_url);
        let enc_key = urlencoding::encode(&key);
        let path = format!("/rest/api/2/issue/{}/transitions", enc_key);
        if self.policy.dry_run {
            // 不预发 GET 查询 transition id,仅展示意图
            let intent = json!({ "transition": { "status": status } });
            return Ok(crate::module::preview_json(
                "jira.transition",
                "POST",
                &path,
                &key,
                Some(&intent),
                Some("只读预览:将以目标状态名实时解析 transition id,未真正执行。确认执行请追加 --confirm"),
            ));
        }
        crate::module::require_confirmed(&self.policy)?;
        let meta = self
            .http
            .get(&format!("/rest/api/2/issue/{}/transitions", enc_key))
            .await?;

        let trans_id = meta["transitions"]
            .as_array()
            .and_then(|arr| {
                arr.iter()
                    .find(|t| {
                        t["name"]
                            .as_str()
                            .map(|n| n.eq_ignore_ascii_case(status))
                            .unwrap_or(false)
                    })
            })
            .and_then(|t| t["id"].as_str())
            .ok_or_else(|| AppError::not_found(format!("未找到匹配的状态: '{}'。请检查可用流转状态名", status)))?;

        let raw = self
            .http
            .post(
                &path,
                json!({ "transition": { "id": trans_id } }),
            )
            .await?;
        if crate::module::is_replayed(&raw) {
            return Ok(raw);
        }

        Ok(json!({
            "status": "success",
            "issue": key,
            "new_status": status,
        }))
    }

    /// GET /rest/api/2/issue/{key}/transitions (查询单子当前所有合法的下一步流转动作与目标状态)
    pub async fn get_transitions(&self, key_or_url: &str) -> Result<Value, AppError> {
        let key = parse_jira_key(key_or_url);
        let path = format!("/rest/api/2/issue/{}/transitions", urlencoding::encode(&key));

        let raw = self.http.get(&path).await?;
        let transitions = raw["transitions"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .map(|t| {
                        json!({
                            "id": t["id"].as_str().unwrap_or(""),
                            "name": t["name"].as_str().unwrap_or(""),
                            "to_status": t["to"]["name"].as_str().unwrap_or(""),
                            "to_status_category": t["to"]["statusCategory"]["name"].as_str().unwrap_or(""),
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Ok(json!({
            "issue_key": key,
            "count": transitions.len(),
            "transitions": transitions,
            "url": format!("{}/browse/{}", self.http.base_url(), key),
        }))
    }

    /// POST /rest/api/2/issueLink (建立两个 Jira 工单之间的关联关系)
    pub async fn link_issue(
        &self,
        from_key_or_url: &str,
        to_key_or_url: &str,
        link_type: &str,
        comment: Option<&str>,
    ) -> Result<Value, AppError> {
        let from_key = parse_jira_key(from_key_or_url);
        let to_key = parse_jira_key(to_key_or_url);

        let mut body = json!({
            "type": {
                "name": link_type.trim()
            },
            "inwardIssue": {
                "key": from_key
            },
            "outwardIssue": {
                "key": to_key
            }
        });

        if let Some(c) = comment {
            let trimmed = c.trim();
            if !trimmed.is_empty() {
                body["comment"] = json!({
                    "body": trimmed
                });
            }
        }

        if self.policy.dry_run {
            let target = format!("{} -> {}", from_key, to_key);
            return Ok(crate::module::preview_json(
                "jira.link",
                "POST",
                "/rest/api/2/issueLink",
                &target,
                Some(&body),
                None,
            ));
        }
        crate::module::require_confirmed(&self.policy)?;
        let raw = self.http.post("/rest/api/2/issueLink", body).await?;
        if crate::module::is_replayed(&raw) {
            return Ok(raw);
        }

        Ok(json!({
            "status": "success",
            "type": link_type.trim(),
            "from_issue": from_key,
            "to_issue": to_key,
            "comment": comment.unwrap_or(""),
            "url": format!("{}/browse/{}", self.http.base_url(), from_key),
        }))
    }
}
