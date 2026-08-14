use std::fs;
use std::io::{IsTerminal, Write};
use std::path::PathBuf;

use anyhow::{anyhow, bail, Context, Result};
use rpassword::prompt_password;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::http::HttpClient;

const CONFIG_DIR_NAME: &str = ".atlassian-cli";
const CONFIG_FILE_NAME: &str = "config.json";

const MODULES: [&str; 3] = ["jira", "confluence", "bitbucket"];

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

    for url in [
        &mut cfg.jira_url,
        &mut cfg.confluence_url,
        &mut cfg.bitbucket_url,
    ] {
        *url = normalize_url(url);
    }

    Ok(cfg)
}

/// 智能 URL 归一化: 自动去除前后空格、补全 https:// 前缀、删除末尾斜杠、智能裁剪网页路径 (如 /browse/..., /pages/..., /projects/...)
pub fn normalize_module_url(input: &str, module: &str) -> String {
    let mut trimmed = input.trim().to_string();
    if trimmed.is_empty() {
        return String::new();
    }
    if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
        trimmed = format!("https://{}", trimmed);
    }
    if let Some(pos) = trimmed.find('?') {
        trimmed.truncate(pos);
    }
    if let Some(pos) = trimmed.find('#') {
        trimmed.truncate(pos);
    }
    while trimmed.ends_with('/') {
        trimmed.pop();
    }

    let markers: &[&str] = match module.to_lowercase().as_str() {
        "jira" => &["/browse/", "/secure/", "/projects/", "/issues/", "/rest/api/"],
        "confluence" => &["/pages/", "/display/", "/spaces/", "/rest/api/"],
        "bitbucket" => &["/projects/", "/users/", "/scm/", "/plugins/servlet/", "/rest/api/"],
        _ => &["/browse/", "/pages/", "/display/", "/projects/"],
    };

    for marker in markers {
        if let Some(pos) = trimmed.find(marker) {
            trimmed.truncate(pos);
            break;
        }
    }

    while trimmed.ends_with('/') {
        trimmed.pop();
    }
    trimmed
}

pub fn normalize_url(input: &str) -> String {
    normalize_module_url(input, "")
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

/// 针对单个模块实时探测凭据有效性并返回可读用户名
#[allow(dead_code)]
pub async fn probe_module_credential(
    module: &str,
    url: &str,
    token: &str,
    allow_insecure: bool,
) -> Result<String> {
    let (user, _) = probe_module_credential_with_healing(module, url, token, allow_insecure).await?;
    Ok(user)
}

/// 探测凭据有效性，并在失败时自动尝试私有部署常见子路径 (如 /jira, /confluence, /wiki, /bitbucket)
pub async fn probe_module_credential_with_healing(
    module: &str,
    url: &str,
    token: &str,
    allow_insecure: bool,
) -> Result<(String, Option<String>)> {
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
) -> Result<String> {
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
        _ => bail!("未知模块: {}", module),
    }
}

/// 交互式初始化: 引导配置 URL 与 Token，边填边实时探测验证凭据 (支持指定单模块)
pub async fn init_interactive(target_module: Option<&str>) -> Result<()> {
    let mut cfg = if config_path().exists() { load()? } else { Config::default() };

    let target_modules: Vec<&str> = if let Some(m) = target_module {
        let m_lower = m.trim().to_lowercase();
        if !MODULES.contains(&m_lower.as_str()) {
            bail!("未知模块: {} (可选: jira / confluence / bitbucket)", m);
        }
        vec![MODULES.iter().find(|&&x| x == m_lower.as_str()).cloned().unwrap()]
    } else {
        MODULES.to_vec()
    };

    println!("=== atlassian-cli 配置模式 ===");
    println!("说明: 请配置所需 Atlassian 产品的 Base URL 与 PAT Token。");
    println!("配置仅保存在本地 {} (权限 0600)。\n", config_path().display());

    for module in target_modules {
        println!(">>> 配置 {} 模块", module.to_uppercase());
        println!("  💡 提示: 直接从浏览器地址栏复制任意 {} 页面网址粘贴即可 (CLI 会自动智能清洗提取 Base URL)", module);
        let example = match module {
            "jira" => "https://jira.company.com 或 https://company.com/jira/browse/PROJ-123",
            "confluence" => "https://confluence.company.com 或 https://company.com/confluence/pages/viewpage.action?pageId=123",
            _ => "https://bitbucket.company.com 或 https://company.com/bitbucket/projects/PROJ/repos/repo",
        };
        println!("  示例: {}", example);

        // 1. Base URL 配置
        let cur_url = match module {
            "jira" => &cfg.jira_url,
            "confluence" => &cfg.confluence_url,
            _ => &cfg.bitbucket_url,
        };

        let prompt_url = if cur_url.is_empty() {
            format!("Enter {} Base URL (留空跳过)", module)
        } else {
            format!("Enter {} Base URL (已配置: {}, 留空保留)", module, cur_url)
        };

        let raw_url = input_line(&prompt_url)?;
        let mut url = if raw_url.is_empty() {
            cur_url.clone()
        } else {
            normalize_module_url(&raw_url, module)
        };

        if url.is_empty() {
            println!("  [{}] 已跳过 URL 配置。\n", module);
            continue;
        }

        match module {
            "jira" => cfg.jira_url = url.clone(),
            "confluence" => cfg.confluence_url = url.clone(),
            _ => cfg.bitbucket_url = url.clone(),
        }

        // 2. Token 配置与实时验证循环
        let cur_token = match module {
            "jira" => &cfg.jira_token,
            "confluence" => &cfg.confluence_token,
            _ => &cfg.bitbucket_token,
        };

        let mut token = cur_token.clone();

        loop {
            let prompt_tok = if token.is_empty() {
                format!("Enter {} PAT Token (暗显输入, 留空跳过)", module)
            } else {
                format!("Enter {} PAT Token (暗显输入, 已配置, 留空保留/测试)", module)
            };

            let input_tok = prompt_token(&prompt_tok)?;
            if !input_tok.is_empty() {
                token = input_tok;
            }

            if token.is_empty() {
                println!("  [{}] 已跳过 Token 配置。\n", module);
                break;
            }

            print!("  正在实时测试 {} 凭据 ({}) ... ", module, url);
            std::io::stdout().flush().ok();

            match probe_module_credential_with_healing(module, &url, &token, cfg.allow_insecure_certs).await {
                Ok((user_name, healed_url_opt)) => {
                    if let Some(healed_url) = healed_url_opt {
                        println!("SUCCESS (已认证: {})", user_name);
                        println!("  💡 智能自愈: 检测到您的私有部署使用了子路径，已自动更正 Base URL 为: {}", healed_url);
                        url = healed_url;
                    } else {
                        println!("SUCCESS (已认证: {})", user_name);
                    }
                    match module {
                        "jira" => {
                            cfg.jira_url = url.clone();
                            cfg.jira_token = token.clone();
                        }
                        "confluence" => {
                            cfg.confluence_url = url.clone();
                            cfg.confluence_token = token.clone();
                        }
                        _ => {
                            cfg.bitbucket_url = url.clone();
                            cfg.bitbucket_token = token.clone();
                        }
                    }
                    break;
                }
                Err(e) => {
                    println!("FAILED ({})", e);
                    let retry_ans = input_line("  凭据验证失败，是否重新输入 Token? [Y/n]")?;
                    if retry_ans.eq_ignore_ascii_case("n") {
                        println!("  [{}] 已跳过该 Token 校验。\n", module);
                        break;
                    }
                }
            }
        }
        println!();
    }

    save(&cfg)?;
    println!("✅ 所有配置更新并保存完成！");
    Ok(())
}

/// 探测已配置模块的网络连通性与 PAT 登录身份信息
pub async fn test(cfg: &Config) -> Result<Value> {
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

/// 读 token:终端下暗显输入;非终端(管道/CI)退化为普通读取
fn prompt_token(prompt: &str) -> Result<String> {
    if std::io::stdin().is_terminal() {
        Ok(prompt_password(prompt)?.trim().to_string())
    } else {
        print!("{}: ", prompt);
        std::io::stdout().flush().ok();
        let mut line = String::new();
        std::io::stdin().read_line(&mut line)?;
        Ok(line.trim().to_string())
    }
}

/// 交互式设置单个模块的 Token:本机终端暗显输入,不经任何日志/网络
/// 这是 Agent 场景下最安全的凭据传递方式——token 只在终端和配置文件间流转
pub fn token_interactive(module: &str) -> Result<()> {
    if !MODULES.contains(&module) {
        bail!("未知模块: {} (可选: jira / confluence / bitbucket)", module);
    }
    let v = prompt_token(&format!("Enter {} Token", module))?;
    if v.is_empty() {
        bail!("输入为空,未做修改");
    }
    set_token(module, &v)
}

/// 从标准输入读取 Token 并设置 (适合 Agent / CI 避免命令行明文泄露)
pub fn set_token_from_stdin(module: &str) -> Result<()> {
    if !MODULES.contains(&module) {
        bail!("未知模块: {} (可选: jira / confluence / bitbucket)", module);
    }
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    let token = line.trim();
    if token.is_empty() {
        bail!("标准输入读取的 token 为空");
    }
    set_token(module, token)
}

/// 非交互设置某个模块的 Token(适合脚本 / CI 注入)
pub fn set_token(module: &str, token: &str) -> Result<()> {
    let clean_token = token.trim();
    if clean_token.is_empty() {
        bail!("token 不能为空");
    }
    let mut cfg = load()?;
    match module {
        "jira" => cfg.jira_token = clean_token.to_string(),
        "confluence" => cfg.confluence_token = clean_token.to_string(),
        "bitbucket" => cfg.bitbucket_token = clean_token.to_string(),
        _ => bail!("未知模块: {} (可选: jira / confluence / bitbucket)", module),
    }
    save(&cfg)
}

/// 设置某个模块的 Base URL (自动 normalize 补全 protocol 并去除尾部斜杠)
pub fn set_url(module: &str, url: &str) -> Result<()> {
    let clean_url = normalize_url(url);
    if clean_url.is_empty() {
        bail!("url 不能为空");
    }
    let mut cfg = load()?;
    match module {
        "jira" => cfg.jira_url = clean_url,
        "confluence" => cfg.confluence_url = clean_url,
        "bitbucket" => cfg.bitbucket_url = clean_url,
        _ => bail!("未知模块: {} (可选: jira / confluence / bitbucket)", module),
    }
    save(&cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_url() {
        assert_eq!(normalize_url("jira.example.com"), "https://jira.example.com");
        assert_eq!(normalize_url("  jira.example.com/  "), "https://jira.example.com");
        assert_eq!(normalize_url("http://jira.example.com/jira/"), "http://jira.example.com/jira");
        assert_eq!(normalize_url("https://gitpub.company.com"), "https://gitpub.company.com");
    }

    #[test]
    fn test_normalize_module_url_smart_strip() {
        // Jira 单子 URL
        assert_eq!(
            normalize_module_url("https://jira.company.com/browse/PROJ-123", "jira"),
            "https://jira.company.com"
        );
        assert_eq!(
            normalize_module_url("https://company.com/jira/browse/PROJSA-123?filter=1", "jira"),
            "https://company.com/jira"
        );
        assert_eq!(
            normalize_module_url("https://jira.company.com/secure/Dashboard.jspa", "jira"),
            "https://jira.company.com"
        );

        // Confluence 页面 URL
        assert_eq!(
            normalize_module_url("https://confluence.company.com/pages/viewpage.action?pageId=123", "confluence"),
            "https://confluence.company.com"
        );
        assert_eq!(
            normalize_module_url("https://company.com/confluence/display/SPACE/Title", "confluence"),
            "https://company.com/confluence"
        );

        // Bitbucket PR 与仓库 URL
        assert_eq!(
            normalize_module_url("https://bitbucket.company.com/projects/PROJ/repos/repo/pull-requests/1", "bitbucket"),
            "https://bitbucket.company.com"
        );
        assert_eq!(
            normalize_module_url("https://company.com/bitbucket/projects/PROJ/repos/repo", "bitbucket"),
            "https://company.com/bitbucket"
        );
    }
}

/// 清除某个模块的凭据与配置
pub fn unset(module: &str) -> Result<()> {
    if !MODULES.contains(&module) {
        bail!("未知模块: {} (可选: jira / confluence / bitbucket)", module);
    }
    let mut cfg = load()?;
    match module {
        "jira" => {
            cfg.jira_url.clear();
            cfg.jira_token.clear();
        }
        "confluence" => {
            cfg.confluence_url.clear();
            cfg.confluence_token.clear();
        }
        "bitbucket" => {
            cfg.bitbucket_url.clear();
            cfg.bitbucket_token.clear();
        }
        _ => unreachable!(),
    }
    save(&cfg)
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

fn input_line(prompt: &str) -> Result<String> {
    print!("{}: ", prompt);
    std::io::stdout().flush().ok();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    Ok(line.trim().to_string())
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
            "缺少 {}_URL。请先运行 `atlassian-cli config init` 或设置环境变量",
            which.to_uppercase()
        ));
    }
    if token.trim().is_empty() {
        return Err(anyhow!(
            "缺少 {}_TOKEN。请运行 `atlassian-cli config init` 或 `atlassian-cli config set-token {} --stdin`",
            which.to_uppercase(),
            which
        ));
    }
    Ok(())
}
