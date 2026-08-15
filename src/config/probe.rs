use crate::error::AppError;
use serde_json::{json, Value};

use super::Config;
use crate::http::HttpClient;

/// 针对单个模块实时探测凭据有效性并返回可读用户名
#[allow(dead_code)]
pub async fn probe_module_credential(
    module: &str,
    url: &str,
    token: &str,
    allow_insecure: bool,
) -> Result<String, AppError> {
    let (user, _) = probe_module_credential_with_healing(module, url, token, allow_insecure).await?;
    Ok(user)
}

/// 探测凭据有效性，并在失败时自动尝试私有部署常见子路径 (如 /jira, /confluence, /wiki, /bitbucket)
pub async fn probe_module_credential_with_healing(
    module: &str,
    url: &str,
    token: &str,
    allow_insecure: bool,
) -> Result<(String, Option<String>), AppError> {
    match probe_single_url(module, url, token, allow_insecure).await {
        Ok(user) => Ok((user, None)),
        Err(orig_err) => {
            let candidates: Vec<String> = match module {
                "jira" => {
                    if !url.ends_with("/jira") {
                        vec![format!("{}/jira", url)]
                    } else {
                        vec![]
                    }
                }
                "confluence" => {
                    let mut list = Vec::new();
                    if !url.ends_with("/confluence") {
                        list.push(format!("{}/confluence", url));
                    }
                    if !url.ends_with("/wiki") {
                        list.push(format!("{}/wiki", url));
                    }
                    list
                }
                "bitbucket" => {
                    let mut list = Vec::new();
                    if !url.ends_with("/bitbucket") {
                        list.push(format!("{}/bitbucket", url));
                    }
                    if !url.ends_with("/stash") {
                        list.push(format!("{}/stash", url));
                    }
                    list
                }
                _ => vec![],
            };

            for candidate in candidates {
                if let Ok(user) = probe_single_url(module, &candidate, token, allow_insecure).await {
                    return Ok((user, Some(candidate)));
                }
            }

            Err(orig_err)
        }
    }
}

async fn probe_single_url(
    module: &str,
    url: &str,
    token: &str,
    allow_insecure: bool,
) -> Result<String, AppError> {
    let client = HttpClient::new(url.to_string(), token, allow_insecure)?;
    match module {
        "jira" => {
            let user = client.get("/rest/api/2/myself").await?;
            let name = user["displayName"]
                .as_str()
                .or(user["name"].as_str())
                .unwrap_or("已认证");
            Ok(name.to_string())
        }
        "confluence" => {
            let user = client.get("/rest/api/user/current").await?;
            let name = user["displayName"]
                .as_str()
                .or(user["username"].as_str())
                .unwrap_or("已认证");
            Ok(name.to_string())
        }
        "bitbucket" => {
            if let Ok(raw_uname) = client.get_text("/plugins/servlet/applinks/whoami").await {
                let uname = raw_uname.trim();
                if !uname.is_empty() {
                    if let Ok(raw) = client
                        .get_with_query("/rest/api/1.0/users", &[("filter", uname)])
                        .await
                    {
                        if let Some(dname) = raw["values"][0]["displayName"].as_str() {
                            return Ok(dname.to_string());
                        }
                    }
                    return Ok(uname.to_string());
                }
            }
            client.get("/rest/api/1.0/projects?limit=1").await?;
            Ok("已连通".to_string())
        }
        _ => return Err(AppError::param_invalid(format!("未知模块: {}", module))),
    }
}

/// 探测已配置模块的网络连通性与 PAT 登录身份信息
pub async fn test(cfg: &Config) -> Result<Value, AppError> {
    use std::io::Write;
    println!("=== 正在测试 Atlassian 服务连通性与 Token 有效性 ===");
    let mut results = serde_json::Map::new();

    // 1. Jira
    if !cfg.jira_url.is_empty() && !cfg.jira_token.trim().is_empty() {
        print!("  Checking Jira ({}) ... ", cfg.jira_url);
        std::io::stdout().flush().ok();
        match HttpClient::new(cfg.jira_url.clone(), &cfg.jira_token, cfg.allow_insecure_certs) {
            Ok(client) => match client.get("/rest/api/2/myself").await {
                Ok(user) => {
                    let name = user["displayName"].as_str().or(user["name"].as_str()).unwrap_or("已认证");
                    println!("SUCCESS (User: {})", name);
                    results.insert("jira".to_string(), json!({ "status": "ok", "user": name }));
                }
                Err(e) => {
                    println!("FAILED ({})", e);
                    results.insert("jira".to_string(), json!({ "status": "error", "message": e.to_string() }));
                }
            },
            Err(e) => {
                println!("FAILED ({})", e);
                results.insert("jira".to_string(), json!({ "status": "error", "message": e.to_string() }));
            }
        }
    } else {
        println!("  Checking Jira ... SKIPPED (未配置 URL 或 Token)");
        results.insert("jira".to_string(), json!({ "status": "skipped", "reason": "未配置" }));
    }

    // 2. Confluence
    if !cfg.confluence_url.is_empty() && !cfg.confluence_token.trim().is_empty() {
        print!("  Checking Confluence ({}) ... ", cfg.confluence_url);
        std::io::stdout().flush().ok();
        match HttpClient::new(cfg.confluence_url.clone(), &cfg.confluence_token, cfg.allow_insecure_certs) {
            Ok(client) => match client.get("/rest/api/user/current").await {
                Ok(user) => {
                    let name = user["displayName"].as_str().or(user["username"].as_str()).unwrap_or("已认证");
                    println!("SUCCESS (User: {})", name);
                    results.insert("confluence".to_string(), json!({ "status": "ok", "user": name }));
                }
                Err(e) => {
                    println!("FAILED ({})", e);
                    results.insert("confluence".to_string(), json!({ "status": "error", "message": e.to_string() }));
                }
            },
            Err(e) => {
                println!("FAILED ({})", e);
                results.insert("confluence".to_string(), json!({ "status": "error", "message": e.to_string() }));
            }
        }
    } else {
        println!("  Checking Confluence ... SKIPPED (未配置 URL 或 Token)");
        results.insert("confluence".to_string(), json!({ "status": "skipped", "reason": "未配置" }));
    }

    // 3. Bitbucket
    if !cfg.bitbucket_url.is_empty() && !cfg.bitbucket_token.trim().is_empty() {
        print!("  Checking Bitbucket ({}) ... ", cfg.bitbucket_url);
        std::io::stdout().flush().ok();
        match HttpClient::new(cfg.bitbucket_url.clone(), &cfg.bitbucket_token, cfg.allow_insecure_certs) {
            Ok(client) => {
                let user_res = async {
                    let raw_uname = client.get_text("/plugins/servlet/applinks/whoami").await?;
                    let uname = raw_uname.trim();
                    if uname.is_empty() {
                        anyhow::bail!("whoami returned empty username");
                    }
                    let raw = client.get_with_query("/rest/api/1.0/users", &[("filter", uname)]).await?;
                    let dname = raw["values"][0]["displayName"].as_str().unwrap_or(uname);
                    Ok::<String, anyhow::Error>(dname.to_string())
                }.await;

                match user_res {
                    Ok(name) => {
                        println!("SUCCESS (User: {})", name);
                        results.insert("bitbucket".to_string(), json!({ "status": "ok", "user": name }));
                    }
                    Err(_) => match client.get("/rest/api/1.0/projects?limit=1").await {
                        Ok(_) => {
                            println!("SUCCESS (已连通)");
                            results.insert("bitbucket".to_string(), json!({ "status": "ok" }));
                        }
                        Err(e) => {
                            println!("FAILED ({})", e);
                            results.insert("bitbucket".to_string(), json!({ "status": "error", "message": e.to_string() }));
                        }
                    },
                }
            }
            Err(e) => {
                println!("FAILED ({})", e);
                results.insert("bitbucket".to_string(), json!({ "status": "error", "message": e.to_string() }));
            }
        }
    } else {
        println!("  Checking Bitbucket ... SKIPPED (未配置 URL 或 Token)");
        results.insert("bitbucket".to_string(), json!({ "status": "skipped", "reason": "未配置" }));
    }

    Ok(Value::Object(results))
}
