use serde_json::{json, Value};

use crate::error::AppError;

/// 当前版本(编译期注入 Cargo.toml version)
pub fn current_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// 语义版本比较:a > b ?(按数字段比较,忽略非数字后缀)
fn version_gt(a: &str, b: &str) -> bool {
    let pa: Vec<u64> = a.split('.').filter_map(|s| s.parse().ok()).collect();
    let pb: Vec<u64> = b.split('.').filter_map(|s| s.parse().ok()).collect();
    for i in 0..pa.len().max(pb.len()) {
        let x = pa.get(i).copied().unwrap_or(0);
        let y = pb.get(i).copied().unwrap_or(0);
        if x != y {
            return x > y;
        }
    }
    false
}

/// 检查 GitHub 最新 Release 版本。
///
/// 这是唯一一个向非公司服务器发起的请求(仅 GET 公开 release 元数据,
/// 不携带任何 token/配置/用户数据),且只在用户显式运行 `check-update`
/// 时触发。网络失败降级为 `status: unknown`(非错误——版本检查失败
/// 不代表操作失败)。
pub async fn check_update() -> Result<Value, AppError> {
    let current = current_version();
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(e) => return Ok(unknown(&current, &format!("创建 HTTP 客户端失败: {}", e))),
    };

    let resp = client
        .get("https://api.github.com/repos/infinitezerone/atlassian-cli/releases/latest")
        .header("User-Agent", "atlassian-cli")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await;

    match resp {
        Ok(r) if r.status().is_success() => {
            let v: Value = match r.json().await {
                Ok(j) => j,
                Err(e) => return Ok(unknown(&current, &format!("解析 GitHub 响应失败: {}", e))),
            };
            let latest = v["tag_name"]
                .as_str()
                .unwrap_or("")
                .trim_start_matches('v')
                .to_string();
            let up_to_date = latest.is_empty() || !version_gt(&latest, &current);
            Ok(json!({
                "status": "ok",
                "current_version": current,
                "latest_version": latest,
                "up_to_date": up_to_date,
                "release_url": v["html_url"],
                "hint": if up_to_date {
                    json!("已是最新版本")
                } else {
                    json!(format!(
                        "发现新版本 v{}。升级: curl -fsSL https://cdn.jsdelivr.net/gh/infinitezerone/atlassian-cli@main/install.sh | sh (或 brew upgrade atlassian-cli)",
                        latest
                    ))
                },
            }))
        }
        Ok(r) => Ok(unknown(&current, &format!("GitHub API 返回 HTTP {}", r.status()))),
        Err(e) => Ok(unknown(&current, &format!("无法访问 GitHub API: {}", e))),
    }
}

fn unknown(current: &str, reason: &str) -> Value {
    json!({
        "status": "unknown",
        "current_version": current,
        "latest_version": null,
        "up_to_date": null,
        "hint": format!("无法检查更新: {}", reason),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_gt() {
        assert!(version_gt("0.3.1", "0.3.0"));
        assert!(version_gt("1.0.0", "0.9.9"));
        assert!(version_gt("0.10.0", "0.9.0"));
        assert!(!version_gt("0.3.0", "0.3.0"));
        assert!(!version_gt("0.2.9", "0.3.0"));
        assert!(!version_gt("0.3.0", "0.3.1"));
    }

    #[test]
    fn test_current_version_present() {
        let v = current_version();
        assert!(!v.is_empty());
        assert!(v.split('.').count() >= 2);
    }

    #[test]
    fn test_unknown_structure() {
        let u = unknown("0.3.0", "网络不可达");
        assert_eq!(u["status"], "unknown");
        assert_eq!(u["current_version"], "0.3.0");
        assert!(u["hint"].as_str().unwrap().contains("网络不可达"));
    }
}
