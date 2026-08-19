use atlassian_cli::error::AppError;
use atlassian_cli::module::{AtlassianModule, WritePolicy};
use atlassian_cli::*;

use std::process::exit;

use clap::{CommandFactory, Parser};
use serde_json::{json, Value};

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
            eprintln!("{}", serde_json::to_string(&err.to_json()).unwrap());
            exit(err.code.exit_code());
        }
    };
    let mut cfg = match config::load() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{}", json_output(&e.to_json(), cli.pretty));
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
                SkillSubActions::Uninstall => skill::uninstall_skill(),
            }
        }
        Commands::Schema { command } => schema::render(&Cli::command(), &command),
        Commands::Audit { limit } => audit::read(limit as usize),
        Commands::CheckUpdate => update::check_update().await,
    };

    match result {
        Ok(mut v) => {
            // 防提示注入:清洗所有服务器可控文本字段(工单描述/评论/页面正文/PR 评论/diff 等)
            if security::sanitize_all_strings(&mut v) {
                v["sanitized"] = json!(true);
            }
            println!("{}", json_output(&v, cli.pretty));
        }
        Err(e) => {
            eprintln!("{}", json_output(&e.to_json(), cli.pretty));
            exit(e.code.exit_code());
        }
    }
}

/// 按 --pretty 选择 JSON 序列化格式:默认单行紧凑 JSON (最省 Token),指定 --pretty 则格式化输出
fn json_output(v: &serde_json::Value, pretty: bool) -> String {
    if pretty {
        serde_json::to_string_pretty(v).unwrap_or_else(|_| "{}".to_string())
    } else {
        serde_json::to_string(v).unwrap_or_else(|_| "{}".to_string())
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
