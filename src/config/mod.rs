pub mod interactive;
pub mod probe;
pub mod url;

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::AppError;

pub use interactive::*;
pub use probe::*;
pub use url::*;

const CONFIG_DIR_NAME: &str = ".atlassian-cli";
const CONFIG_FILE_NAME: &str = "config.json";

pub const MODULES: [&str; 3] = ["jira", "confluence", "bitbucket"];

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub jira_url: String,
    pub jira_token: String,
    pub confluence_url: String,
    pub confluence_token: String,
    pub bitbucket_url: String,
    pub bitbucket_token: String,
    pub allow_insecure_certs: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            jira_url: String::new(),
            jira_token: String::new(),
            confluence_url: String::new(),
            confluence_token: String::new(),
            bitbucket_url: String::new(),
            bitbucket_token: String::new(),
            allow_insecure_certs: false,
        }
    }
}

pub fn config_dir() -> PathBuf {
    let home = dirs_next::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(CONFIG_DIR_NAME)
}

pub fn config_path() -> PathBuf {
    let path = config_dir().join(CONFIG_FILE_NAME);
    if let Ok(abs) = fs::canonicalize(&path) {
        abs
    } else {
        path
    }
}

/// 读取配置,优先级:环境变量 > config.json 字段
pub fn load() -> Result<Config, AppError> {
    let path = config_path();
    let mut cfg = if path.exists() {
        let content = fs::read_to_string(&path)
            .map_err(|e| AppError::config_missing(format!("读取配置失败 ({}): {}", path.display(), e)))?;
        serde_json::from_str::<Config>(&content)
            .map_err(|e| AppError::config_missing(format!("解析配置失败 ({}): {}", path.display(), e)))?
    } else {
        Config::default()
    };

    cfg.jira_url = env_override(cfg.jira_url, "JIRA_URL");
    cfg.confluence_url = env_override(cfg.confluence_url, "CONFLUENCE_URL");
    cfg.bitbucket_url = env_override(cfg.bitbucket_url, "BITBUCKET_URL");
    cfg.jira_token = env_override(cfg.jira_token, "JIRA_TOKEN");
    cfg.confluence_token = env_override(cfg.confluence_token, "CONFLUENCE_TOKEN");
    cfg.bitbucket_token = env_override(cfg.bitbucket_token, "BITBUCKET_TOKEN");

    if let Ok(val) = std::env::var("ALLOW_INSECURE_CERTS") {
        if val == "1" || val.eq_ignore_ascii_case("true") {
            cfg.allow_insecure_certs = true;
        }
    }

    for (url, module) in [
        (&mut cfg.jira_url, "jira"),
        (&mut cfg.confluence_url, "confluence"),
        (&mut cfg.bitbucket_url, "bitbucket"),
    ] {
        *url = normalize_module_url(url, module);
    }

    Ok(cfg)
}

/// 保存配置并强制收紧权限:目录 700、文件 600(仅当前用户可读写)
pub fn save(cfg: &Config) -> Result<(), AppError> {
    let dir = config_dir();
    fs::create_dir_all(&dir).map_err(|e| AppError::generic(e.to_string()))?;
    let path = config_path();
    let body =
        serde_json::to_string_pretty(cfg).map_err(|e| AppError::generic(format!("配置序列化失败: {}", e)))?;
    fs::write(&path, body).map_err(|e| AppError::generic(e.to_string()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))
            .map_err(|e| AppError::generic(e.to_string()))?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
            .map_err(|e| AppError::generic(e.to_string()))?;
    }

    eprintln!("Configuration saved to {}", path.display());
    Ok(())
}
/// 打印配置状态(token 打码显示)。
/// stdout 只输出 JSON(供 AI agent 解析);人类可读摘要走 stderr。
pub fn status(cfg: &Config) -> Result<Value, AppError> {
    let mut modules = serde_json::Map::new();
    for module in MODULES {
        let (url, token) = match module {
            "jira" => (&cfg.jira_url, &cfg.jira_token),
            "confluence" => (&cfg.confluence_url, &cfg.confluence_token),
            _ => (&cfg.bitbucket_url, &cfg.bitbucket_token),
        };
        modules.insert(
            module.to_string(),
            json!({
                "url": url,
                "token_configured": !token.is_empty(),
                "token_masked": if token.is_empty() { String::new() } else { mask(token) },
            }),
        );
    }
    let v = json!({
        "status": "ok",
        "config_path": config_path().display().to_string(),
        "allow_insecure_certs": cfg.allow_insecure_certs,
        "modules": modules,
    });

    // 人类可读摘要走 stderr
    eprintln!("配置文件: {}", v["config_path"].as_str().unwrap_or(""));
    eprintln!("TLS 允许自签名证书 (allow_insecure_certs): {}", cfg.allow_insecure_certs);
    for module in MODULES {
        eprintln!("  {:<12} {}", module, v["modules"][module]);
    }
    Ok(v)
}

fn mask(token: &str) -> String {
    let clean = token.trim();
    if clean.len() <= 8 {
        return "****".to_string();
    }
    format!("{}****{}", &clean[..4], &clean[clean.len() - 4..])
}

/// 环境变量覆盖逻辑:优先使用环境变量,若环境变量未设或为空则回退到 config.json 中的 current 字段
fn env_override(current: String, name: &str) -> String {
    if let Ok(val) = std::env::var(name) {
        let trimmed = val.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    current.trim().to_string()
}

pub fn check_ready(cfg: &Config, which: &str) -> Result<(), AppError> {
    let (url, token) = match which {
        "jira" => (&cfg.jira_url, &cfg.jira_token),
        "confluence" => (&cfg.confluence_url, &cfg.confluence_token),
        "bitbucket" => (&cfg.bitbucket_url, &cfg.bitbucket_token),
        _ => return Err(AppError::param_invalid(format!("未知模块: {}", which))),
    };
    if url.is_empty() {
        return Err(AppError::config_missing(format!(
            "缺少 {}_URL。请先运行 `atlassian-cli login` 或设置环境变量",
            which.to_uppercase()
        )));
    }
    if token.trim().is_empty() {
        return Err(AppError::config_missing(format!(
            "缺少 {}_TOKEN。请运行 `atlassian-cli login` 或设置环境变量",
            which.to_uppercase()
        )));
    }
    Ok(())
}
