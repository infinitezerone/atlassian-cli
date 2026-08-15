mod bitbucket;
mod config;
mod confluence;
mod error;
mod http;
mod jira;
mod module;
mod schema;
mod security;
mod skill;
mod utils;

use std::process::exit;

use clap::{CommandFactory, Parser, Subcommand};
use serde_json::{json, Value};

use error::AppError;
use module::{AtlassianModule, WritePolicy};

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

    /// 写操作仅预览 (打印将执行的请求,不真正调用 API)
    #[arg(long, global = true)]
    dry_run: bool,

    /// 显式确认写操作 (不传则写操作将被拒绝执行)
    #[arg(long, global = true)]
    confirm: bool,

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
    /// 自动管理官方 AI Agent Skill 规约 (一键安装 install / 查看状态 status)
    Skill {
        #[command(subcommand)]
        action: Option<SkillSubActions>,
    },
    /// 输出机器可读的命令树 JSON (供 AI Agent 运行时自省),支持子路径过滤,如: schema jira comment
    Schema {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },
}

#[derive(Subcommand)]
enum SkillSubActions {
    /// 自动安装/更新官方 Agent Skill 到全局配置目录 (~/.gemini/config/skills/atlassian-cli/SKILL.md)
    Install,
    /// 检查 Agent Skill 的部署状态与文件路径
    Status,
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
    // clap 解析:help/version 正常输出文本;其余错误转结构化 JSON(exit 2)
    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(e) => {
            use clap::error::ErrorKind;
            if matches!(e.kind(), ErrorKind::DisplayHelp | ErrorKind::DisplayVersion) {
                e.print().unwrap_or(());
                exit(0);
            }
            let err = AppError::param_invalid(format!("参数解析失败: {}", e));
            eprintln!("{}", serde_json::to_string_pretty(&err.to_json()).unwrap());
            exit(err.code.exit_code());
        }
    };
    let mut cfg = match config::load() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{}", serde_json::to_string_pretty(&e.to_json()).unwrap());
            exit(e.code.exit_code());
        }
    };

    if cli.insecure {
        cfg.allow_insecure_certs = true;
    }

    let policy = WritePolicy::from_flags(cli.dry_run, cli.confirm);

    // 装配层分发: 顶层快捷命令 + 业务模块 + 高级 config
    let result = match cli.command {
        // 顶层 Setup / Login 入口
        Commands::Setup { module } | Commands::Login { module } => {
            let r: Result<Value, AppError> = (async {
                config::init_interactive(module.as_deref()).await?;
                Ok(json!({ "status": "ok" }))
            })
            .await;
            r
        }
        // 顶层 Status / Whoami 状态与连通性综合查看
        Commands::Status | Commands::Whoami => {
            let r: Result<Value, AppError> = (async {
                let cfg_status = config::status(&cfg)?;
                let details = config::test(&cfg).await?;
                Ok(json!({ "status": "ok", "config": cfg_status, "details": details }))
            })
            .await;
            r
        }
        Commands::Jira { action } => run::<jira::Jira>(&cfg, action, policy).await,
        Commands::Confluence { action } => run::<confluence::Confluence>(&cfg, action, policy).await,
        Commands::Bitbucket { action } => run::<bitbucket::Bitbucket>(&cfg, action, policy).await,
        // 配置管理走独立分支(交互式 / 本地文件操作,不经过 HTTP 模块)
        Commands::Config { action } => handle_config(action, &cfg).await,
        Commands::Skill { action } => {
            match action.unwrap_or(SkillSubActions::Install) {
                SkillSubActions::Install => skill::install_skill(),
                SkillSubActions::Status => skill::skill_status(),
            }
        }
        Commands::Schema { command } => schema::render(&Cli::command(), &command),
    };

    match result {
        Ok(mut v) => {
            // 防提示注入:清洗所有服务器可控文本字段(工单描述/评论/页面正文/PR 评论/diff 等)
            if security::sanitize_all_strings(&mut v) {
                v["sanitized"] = json!(true);
            }
            println!("{}", serde_json::to_string_pretty(&v).unwrap());
        }
        Err(e) => {
            eprintln!("{}", serde_json::to_string_pretty(&e.to_json()).unwrap());
            exit(e.code.exit_code());
        }
    }
}

/// 泛型装配:自动连接任意 AtlassianModule -> 分发 action -> 统一捕获与包装错误前缀
async fn run<M: AtlassianModule>(
    cfg: &config::Config,
    action: M::Action,
    policy: WritePolicy,
) -> Result<Value, AppError> {
    let m = M::connect(cfg, policy).map_err(|e| e.with_module(M::module_name()))?;
    m.handle(action)
        .await
        .map_err(|e| e.with_module(M::module_name()))
}

/// Config 子命令处理(独立于 HTTP 模块的分发,便于单独测试)
async fn handle_config(action: ConfigActions, cfg: &config::Config) -> Result<Value, AppError> {
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
        ConfigActions::Status => config::status(cfg),
        ConfigActions::Test => {
            let res = config::test(cfg).await?;
            Ok(json!({ "status": "ok", "details": res }))
        }
        ConfigActions::Path => Ok(json!({ "status": "ok", "path": config::config_path().display().to_string() })),
    }
}
