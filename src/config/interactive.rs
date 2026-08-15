use std::io::{IsTerminal, Write};

use crate::error::AppError;
use rpassword::prompt_password;

use super::probe::probe_module_credential_with_healing;
use super::url::normalize_module_url;
use super::{config_path, load, save, Config, MODULES};

/// 交互式初始化: 引导配置 URL 与 Token，边填边实时探测验证凭据 (支持指定单模块)
pub async fn init_interactive(target_module: Option<&str>) -> Result<(), AppError> {
    let mut cfg = if config_path().exists() { load()? } else { Config::default() };

    let target_modules: Vec<&str> = if let Some(m) = target_module {
        let m_lower = m.trim().to_lowercase();
        if !MODULES.contains(&m_lower.as_str()) {
            return Err(AppError::param_invalid(format!("未知模块: {} (可选: jira / confluence / bitbucket)", m)));
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

/// 交互式设置单个模块的 Token:本机终端暗显输入,不经任何日志/网络
pub fn token_interactive(module: &str) -> Result<(), AppError> {
    if !MODULES.contains(&module) {
        return Err(AppError::param_invalid(format!("未知模块: {} (可选: jira / confluence / bitbucket)", module)));
    }
    let v = prompt_token(&format!("Enter {} Token", module))?;
    if v.is_empty() {
        return Err(AppError::param_invalid("输入为空,未做修改"));
    }
    set_token(module, &v)
}

/// 从标准输入读取 Token 并设置 (适合 Agent / CI 避免命令行明文泄露)
pub fn set_token_from_stdin(module: &str) -> Result<(), AppError> {
    if !MODULES.contains(&module) {
        return Err(AppError::param_invalid(format!("未知模块: {} (可选: jira / confluence / bitbucket)", module)));
    }
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    let token = line.trim();
    if token.is_empty() {
        return Err(AppError::param_invalid("标准输入读取的 token 为空"));
    }
    set_token(module, token)
}

/// 设置某个模块的 PAT Token
pub fn set_token(module: &str, token: &str) -> Result<(), AppError> {
    let clean_token = token.trim();
    if clean_token.is_empty() {
        return Err(AppError::param_invalid("token 不能为空"));
    }
    let mut cfg = load()?;
    match module {
        "jira" => cfg.jira_token = clean_token.to_string(),
        "confluence" => cfg.confluence_token = clean_token.to_string(),
        "bitbucket" => cfg.bitbucket_token = clean_token.to_string(),
        _ => return Err(AppError::param_invalid(format!("未知模块: {} (可选: jira / confluence / bitbucket)", module))),
    }
    save(&cfg)
}

/// 设置某个模块的 Base URL (自动 normalize 补全 protocol 并去除尾部斜杠)
pub fn set_url(module: &str, url: &str) -> Result<(), AppError> {
    let clean_url = normalize_module_url(url, module);
    if clean_url.is_empty() {
        return Err(AppError::param_invalid("url 不能为空"));
    }
    let mut cfg = load()?;
    match module {
        "jira" => cfg.jira_url = clean_url,
        "confluence" => cfg.confluence_url = clean_url,
        "bitbucket" => cfg.bitbucket_url = clean_url,
        _ => return Err(AppError::param_invalid(format!("未知模块: {} (可选: jira / confluence / bitbucket)", module))),
    }
    save(&cfg)
}

/// 清除某个模块的凭据与配置
pub fn unset(module: &str) -> Result<(), AppError> {
    if !MODULES.contains(&module) {
        return Err(AppError::param_invalid(format!("未知模块: {} (可选: jira / confluence / bitbucket)", module)));
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

fn prompt_token(prompt: &str) -> Result<String, AppError> {
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

fn input_line(prompt: &str) -> Result<String, AppError> {
    print!("{}: ", prompt);
    std::io::stdout().flush().ok();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    Ok(line.trim().to_string())
}
