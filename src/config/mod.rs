pub mod interactive;
pub mod probe;
pub mod url;

use std::fs;
use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

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
pub fn load() -> Result<Config> {
    let path = config_path();
    let mut cfg = if path.exists() {
        let content = fs::read_to_string(&path)
            .with_context(|| format!("读取配置失败: {}", path.display()))?;
        serde_json::from_str::<Config>(&content)
            .with_context(|| format!("解析配置失败: {}", path.display()))?
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
pub fn save(cfg: &Config) -> Result<()> {
    let dir = config_dir();
    fs::create_dir_all(&dir)?;
    let path = config_path();
    fs::write(&path, serde_json::to_string_pretty(cfg)?)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    }

    println!("Configuration saved to {}", path.display());
    Ok(())
}

/// 打印配置状态(token 打码显示)
pub fn status(cfg: &Config) -> Result<()> {
    println!("配置文件: {}", config_path().display());
    println!("TLS 允许自签名证书 (allow_insecure_certs): {}", cfg.allow_insecure_certs);
    for module in MODULES {
        let (url, token) = match module {
            "jira" => (&cfg.jira_url, &cfg.jira_token),
            "confluence" => (&cfg.confluence_url, &cfg.confluence_token),
            _ => (&cfg.bitbucket_url, &cfg.bitbucket_token),
        };
        let url = if url.is_empty() { "(未设置)".to_string() } else { url.clone() };
        let t = if token.is_empty() {
            "未配置".to_string()
        } else {
            format!("已配置 ({})", mask(token))
        };
        println!("  {:<12} url={}  token={}", module, url, t);
    }
    Ok(())
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

pub fn check_ready(cfg: &Config, which: &str) -> Result<()> {
    let (url, token) = match which {
        "jira" => (&cfg.jira_url, &cfg.jira_token),
        "confluence" => (&cfg.confluence_url, &cfg.confluence_token),
        "bitbucket" => (&cfg.bitbucket_url, &cfg.bitbucket_token),
        _ => return Err(anyhow!("未知模块: {}", which)),
    };
    if url.is_empty() {
        return Err(anyhow!(
            "缺少 {}_URL。请先运行 `atlassian-cli login` 或设置环境变量",
            which.to_uppercase()
        ));
    }
    if token.trim().is_empty() {
        return Err(anyhow!(
            "缺少 {}_TOKEN。请运行 `atlassian-cli login` 或设置环境变量",
            which.to_uppercase()
        ));
    }
    Ok(())
}
