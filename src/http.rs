use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use reqwest::Client;
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_retry::{policies::ExponentialBackoff, RetryTransientMiddleware};
use serde_json::Value;

use crate::error::{AppError, ErrorCode};

/// 统一 HTTP 客户端: 封装 3 次指数退避重试 (reqwest-retry)、5s 建连超时、Bearer Token 认证与 URL Query 编码抽象
pub struct HttpClient {
    client: ClientWithMiddleware,
    raw_client: Client,
    base_url: String,
}

impl HttpClient {
    pub fn new(base_url: String, token: &str, allow_insecure_certs: bool) -> Result<Self, AppError> {
        let clean_token = token.trim();
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", clean_token))
                .map_err(|_| AppError::param_invalid("Token 包含非法字符,无法构造 Authorization 头"))?,
        );

        // 1. 基础 Reqwest Client 配置 (5s 建连超时, 45s 响应超时)
        let raw_client = Client::builder()
            .default_headers(headers)
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(45))
            .danger_accept_invalid_certs(allow_insecure_certs)
            .build()?;

        // 2. 指数退避重试策略: 最多重试 3 次 (针对网络闪断、TCP Connection Reset、5xx、429 自动化重试)
        let retry_policy = ExponentialBackoff::builder().build_with_max_retries(3);

        // 3. 构建带 Middleware 的 Client
        let client = ClientBuilder::new(raw_client.clone())
            .with(RetryTransientMiddleware::new_with_policy(retry_policy))
            .build();

        Ok(Self {
            client,
            raw_client,
            base_url,
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// 发起 GET 请求
    pub async fn get(&self, path: &str) -> Result<Value, AppError> {
        let url = self.url(path);
        let res = self.client.get(&url).send().await?;
        Self::parse(res).await
    }

    /// 发起返回纯文本内容的 GET 请求 (适用于 /whoami 等非 JSON 接口)
    pub async fn get_text(&self, path: &str) -> Result<String, AppError> {
        let url = self.url(path);
        let res = self.client.get(&url).send().await?;
        let status = res.status();
        let text = res.text().await.unwrap_or_default();
        if status.is_success() {
            Ok(text)
        } else {
            let cleaned = crate::security::sanitize_external_text(&text.chars().take(300).collect::<String>());
            Err(classify_http_error(status, &cleaned))
        }
    }

    /// 发起带 Query 参数的 GET 请求 (自动对键值进行 urlencoding 编码)
    pub async fn get_with_query(&self, path: &str, query: &[(&str, &str)]) -> Result<Value, AppError> {
        let url = self.build_url_with_query(path, query);
        let res = self.client.get(&url).send().await?;
        Self::parse(res).await
    }

    /// 发起 POST 请求
    pub async fn post(&self, path: &str, body: Value) -> Result<Value, AppError> {
        let url = self.url(path);
        let res = self.client.post(&url).json(&body).send().await?;
        Self::parse(res).await
    }

    /// 发起 PUT 请求
    pub async fn put(&self, path: &str, body: Value) -> Result<Value, AppError> {
        let url = self.url(path);
        let res = self.client.put(&url).json(&body).send().await?;
        Self::parse(res).await
    }

    /// 发起 DELETE 请求
    pub async fn delete(&self, path: &str) -> Result<Value, AppError> {
        let url = self.url(path);
        let res = self.client.delete(&url).send().await?;
        Self::parse(res).await
    }

    /// 发起带文件上传的 Multipart POST 请求 (自动添加 X-Atlassian-Token: nocheck)
    pub async fn post_multipart(&self, path: &str, form: reqwest::multipart::Form) -> Result<Value, AppError> {
        let url = self.url(path);
        let res = self
            .raw_client
            .post(&url)
            .header("X-Atlassian-Token", "nocheck")
            .multipart(form)
            .send()
            .await?;
        Self::parse(res).await
    }

    /// 构建带 Query 参数的安全 URL (键值自动 urlencoding)
    pub fn build_url_with_query(&self, path: &str, query: &[(&str, &str)]) -> String {
        let base = self.url(path);
        if query.is_empty() {
            return base;
        }
        let encoded_pairs: Vec<String> = query
            .iter()
            .map(|(k, v)| format!("{}={}", urlencoding::encode(k), urlencoding::encode(v)))
            .collect();
        format!("{}?{}", base, encoded_pairs.join("&"))
    }

    async fn parse(res: reqwest::Response) -> Result<Value, AppError> {
        let status = res.status();
        let text = res.text().await.unwrap_or_default();
        if status.is_success() {
            if text.trim().is_empty() {
                return Ok(Value::Null);
            }
            serde_json::from_str(&text)
                .map_err(|e| AppError::http_error(format!("响应不是合法 JSON ({}): {}", status, e)))
        } else {
            // 尽量把 Jira/Confluence 的 error message 透传出来(先做提示注入清洗)
            let server_msg = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|v| {
                    v.get("message")
                        .or_else(|| v.get("errors"))
                        .or_else(|| v.get("errorMessages").and_then(|m| m.as_array().and_then(|a| a.first())))
                        .and_then(|m| m.as_str())
                        .map(|s| crate::security::sanitize_external_text(s))
                })
                .unwrap_or_else(|| {
                    crate::security::sanitize_external_text(&text.chars().take(300).collect::<String>())
                });

            let mut err = classify_http_error(status, &server_msg);
            if !server_msg.is_empty() {
                let detail = match status.as_u16() {
                    401 => "认证失败: PAT Token 无效或已过期",
                    403 => "权限拒绝: 当前 Token 无权访问该资源",
                    404 => "资源未找到: 请检查 API 路径或 Base URL 是否包含正确前缀 (如 /jira 或 /confluence)",
                    _ => "",
                };
                if server_msg != detail {
                    err = err.with_detail(server_msg);
                }
            }
            Err(err)
        }
    }
}

/// 纯函数:HTTP 状态码 -> 结构化错误(供 parse/get_text 共用,便于单测)
pub(crate) fn classify_http_error(status: reqwest::StatusCode, _server_msg: &str) -> AppError {
    let (code, detail) = match status.as_u16() {
        401 => (ErrorCode::AuthExpired, "认证失败: PAT Token 无效或已过期"),
        403 => (ErrorCode::PermissionDenied, "权限拒绝: 当前 Token 无权访问该资源"),
        404 => (
            ErrorCode::NotFound,
            "资源未找到: 请检查 API 路径或 Base URL 是否包含正确前缀 (如 /jira 或 /confluence)",
        ),
        _ => (ErrorCode::HttpError, ""),
    };
    let message = if detail.is_empty() {
        format!("HTTP [{}] 请求失败", status)
    } else {
        format!("HTTP [{}] {}", status, detail)
    };
    AppError::new(code, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::StatusCode;

    #[test]
    fn test_classify_http_error_mapping() {
        let e = classify_http_error(StatusCode::UNAUTHORIZED, "");
        assert_eq!(e.code, ErrorCode::AuthExpired);
        assert_eq!(e.code.exit_code(), 10);
        assert!(e.message.contains("401"));

        let e = classify_http_error(StatusCode::FORBIDDEN, "");
        assert_eq!(e.code, ErrorCode::PermissionDenied);
        assert_eq!(e.code.exit_code(), 11);

        let e = classify_http_error(StatusCode::NOT_FOUND, "");
        assert_eq!(e.code, ErrorCode::NotFound);
        assert_eq!(e.code.exit_code(), 20);

        let e = classify_http_error(StatusCode::INTERNAL_SERVER_ERROR, "");
        assert_eq!(e.code, ErrorCode::HttpError);
        assert_eq!(e.code.exit_code(), 1);

        let e = classify_http_error(StatusCode::TOO_MANY_REQUESTS, "");
        assert_eq!(e.code, ErrorCode::HttpError);
    }
}
