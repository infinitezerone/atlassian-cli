use std::time::Duration;

use anyhow::{anyhow, Result};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use reqwest::Client;
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_retry::{policies::ExponentialBackoff, RetryTransientMiddleware};
use serde_json::Value;

/// 统一 HTTP 客户端: 封装 3 次指数退避重试 (reqwest-retry)、5s 建连超时、Bearer Token 认证与 URL Query 编码抽象
pub struct HttpClient {
    client: ClientWithMiddleware,
    base_url: String,
}

impl HttpClient {
    pub fn new(base_url: String, token: &str, allow_insecure_certs: bool) -> Result<Self> {
        let clean_token = token.trim();
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", clean_token))?,
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
        let client = ClientBuilder::new(raw_client)
            .with(RetryTransientMiddleware::new_with_policy(retry_policy))
            .build();

        Ok(Self { client, base_url })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// 发起 GET 请求
    pub async fn get(&self, path: &str) -> Result<Value> {
        let url = self.url(path);
        let res = self.client.get(&url).send().await?;
        Self::parse(res).await
    }

    /// 发起带 Query 参数的 GET 请求 (自动对键值进行 urlencoding 编码)
    pub async fn get_with_query(&self, path: &str, query: &[(&str, &str)]) -> Result<Value> {
        let url = self.build_url_with_query(path, query);
        let res = self.client.get(&url).send().await?;
        Self::parse(res).await
    }

    /// 发起 POST 请求
    pub async fn post(&self, path: &str, body: Value) -> Result<Value> {
        let url = self.url(path);
        let res = self.client.post(&url).json(&body).send().await?;
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

    async fn parse(res: reqwest::Response) -> Result<Value> {
        let status = res.status();
        let text = res.text().await.unwrap_or_default();
        if status.is_success() {
            if text.trim().is_empty() {
                return Ok(Value::Null);
            }
            serde_json::from_str(&text)
                .map_err(|e| anyhow!("响应不是合法 JSON ({}): {}", status, e))
        } else {
            // 尽量把 Jira/Confluence 的 error message 透传出来
            let server_msg = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|v| {
                    v.get("message")
                        .or_else(|| v.get("errors"))
                        .or_else(|| v.get("errorMessages").and_then(|m| m.as_array().and_then(|a| a.first())))
                        .and_then(|m| m.as_str())
                        .map(|s| s.to_string())
                })
                .unwrap_or_else(|| text.chars().take(300).collect());

            let detail = match status.as_u16() {
                401 => "认证失败: PAT Token 无效或已过期",
                403 => "权限拒绝: 当前 Token 无权访问该资源",
                404 => "资源未找到: 请检查 API 路径或 Base URL 是否包含正确前缀 (如 /jira 或 /confluence)",
                _ => server_msg.as_str(),
            };

            if server_msg.is_empty() || server_msg == detail {
                Err(anyhow!("HTTP [{}] {}", status, detail))
            } else {
                Err(anyhow!("HTTP [{}] {} ({})", status, detail, server_msg))
            }
        }
    }
}
