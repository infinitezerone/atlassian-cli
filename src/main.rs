mod bitbucket;
mod config;
mod confluence;
mod http;
mod jira;
mod module;
mod utils;

use std::process::exit;

use anyhow::Result;
use clap::{Parser, Subcommand};
use serde_json::{json, Value};

use module::AtlassianModule;

#[derive(Parser)]
#[command(
    name = "atlassian-cli",
    about = "Atlassian 私有部署统一 AI CLI (Jira + Confluence + Bitbucket)",
    version
)]
struct Cli {
    /// 允许自签名 / 无效 TLS 证书 (不校验 HTTPS 证书合法性)
    #[arg(long, short = 'k', global = true)]
    insecure: bool,

    #[command(subcommand)]
    command: Commands,
}

/// 顶层命令 = 产品清单 + 快捷接入命令
#[derive(Subcommand)]
enum Commands {
    /// 首次接入与配置初始化 (人类交互引导 + 连通性测试)
    Setup {
        /// 可选指定要配置的模块 (jira / confluence / bitbucket)
        module: Option<String>,
    },
    /// 登录接入 (Setup 快捷别名)
    Login {
        /// 可选指定要配置的模块 (jira / confluence / bitbucket)
        module: Option<String>,
    },
    /// 查看配置状态、TLS 开关与服务连通身份 (Whoami)
    Status,
    /// 身份与状态查看 (Status 快捷别名)
    Whoami,

    /// Jira 操作
    Jira {
        #[command(subcommand)]
        action: jira::JiraActions,
    },
    /// Confluence 操作
    Confluence {
        #[command(subcommand)]
        action: confluence::ConfluenceActions,
    },
    /// Bitbucket 操作
    Bitbucket {
        #[command(subcommand)]
        action: bitbucket::BitbucketActions,
    },
    /// 高级配置管理 (set / set-url / unset / path)
    Config {
        #[command(subcommand)]
        action: ConfigActions,
    },
}

#[derive(Subcommand)]
enum ConfigActions {
    /// 全量交互式初始化/更新配置 (人类模式,落盘后自动测试连通性)
    Init,
    /// 设置某模块的 Token: set <module> [--stdin] [TOKEN] (不传 TOKEN 自动开启终端暗显交互)
    Set {
        module: String,
        /// 从标准输入读取 Token (推荐,防止 ps aux / history 泄漏)
        #[arg(long)]
        stdin: bool,
        /// 直接在命令行中指定 Token (不推荐,可能在进程列表明文泄漏)
        token: Option<String>,
    },
    /// 单独设置某模块的 Base URL: set-url <module> <URL>
    SetUrl {
        module: String,
        url: String,
    },
    /// 清除某模块的配置与凭据: unset <module>
    Unset {
        module: String,
    },
    /// 查看配置状态 (Token 打码)
    Status,
    /// 测试已配置服务的连通性与 PAT 认证有效性
    Test,
    /// 打印配置文件路径
    Path,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let mut cfg = match config::load() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{}", json!({ "status": "error", "message": e.to_string() }));
            exit(1);
        }
    };

    if cli.insecure {
        cfg.allow_insecure_certs = true;
    }

    // 装配层分发: 顶层快捷命令 + 业务模块 + 高级 config
    let result = match cli.command {
        // 顶层 Setup / Login 入口
        Commands::Setup { module } | Commands::Login { module } => {
            let r: Result<Value> = (async {
                config::init_interactive(module.as_deref()).await?;
                Ok(json!({ "status": "ok" }))
            })
            .await;
            r
        }
        // 顶层 Status / Whoami 状态与连通性综合查看
        Commands::Status | Commands::Whoami => {
            let r: Result<Value> = (async {
                config::status(&cfg)?;
                println!();
                let details = config::test(&cfg).await?;
                Ok(json!({ "status": "ok", "details": details }))
            })
            .await;
            r
        }
        Commands::Jira { action } => run::<jira::Jira>(&cfg, action).await,
        Commands::Confluence { action } => run::<confluence::Confluence>(&cfg, action).await,
        Commands::Bitbucket { action } => run::<bitbucket::Bitbucket>(&cfg, action).await,
        // 配置管理走独立分支(交互式 / 本地文件操作,不经过 HTTP 模块)
        Commands::Config { action } => {
            let r: Result<Value> = (async {
                match action {
                    ConfigActions::Init => {
                        config::init_interactive(None).await?;
                        Ok(json!({ "status": "ok" }))
                    }
                    ConfigActions::Set { module, stdin, token } => {
                        if stdin {
                            config::set_token_from_stdin(&module)?;
                            Ok(json!({ "status": "ok", "module": module, "token": "已从 stdin 写入 (打码存储)" }))
                        } else if let Some(t) = token {
                            config::set_token(&module, &t)?;
                            Ok(json!({ "status": "ok", "module": module, "token": "已写入 (打码存储)" }))
                        } else {
                            config::token_interactive(&module)?;
                            Ok(json!({ "status": "ok", "module": module, "token": "已写入 (打码存储)" }))
                        }
                    }
                    ConfigActions::SetUrl { module, url } => {
                        config::set_url(&module, &url)?;
                        Ok(json!({ "status": "ok", "module": module, "url": url }))
                    }
                    ConfigActions::Unset { module } => {
                        config::unset(&module)?;
                        Ok(json!({ "status": "ok", "module": module, "message": "配置与凭据已清除" }))
                    }
                    ConfigActions::Status => {
                        config::status(&cfg)?;
                        Ok(json!({ "status": "ok" }))
                    }
                    ConfigActions::Test => {
                        let res = config::test(&cfg).await?;
                        Ok(json!({ "status": "ok", "details": res }))
                    }
                    ConfigActions::Path => {
                        println!("{}", config::config_path().display());
                        Ok(json!({ "status": "ok" }))
                    }
                }
            })
            .await;
            r
        }
    };

    match result {
        Ok(v) => println!("{}", serde_json::to_string_pretty(&v).unwrap()),
        Err(e) => {
            eprintln!("{}", json!({ "status": "error", "message": e.to_string() }));
            exit(1);
        }
    }
}

/// 泛型装配:自动连接任意 AtlassianModule -> 分发 action -> 统一捕获与包装错误前缀
async fn run<M: AtlassianModule>(cfg: &config::Config, action: M::Action) -> Result<Value> {
    let m = M::connect(cfg).map_err(|e| anyhow::anyhow!("[{}] {}", M::module_name(), e))?;
    m.handle(action)
        .await
        .map_err(|e| anyhow::anyhow!("[{}] {}", M::module_name(), e))
}
