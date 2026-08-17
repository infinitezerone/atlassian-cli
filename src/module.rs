use serde_json::{json, Value};

use crate::config::Config;
use crate::error::AppError;

/// 写操作安全策略:由顶层全局 flag (--dry-run / --confirm) 构造。
#[derive(Debug, Clone, Copy)]
pub struct WritePolicy {
    /// 仅预览写操作,不真正执行
    pub dry_run: bool,
    /// 显式确认写操作(未确认则拒绝执行)
    pub confirm: bool,
}

impl WritePolicy {
    /// 从 CLI 全局 flag 构造;兼容逃生阀:
    /// 环境变量 `ATLASSIAN_CLI_ALLOW_UNCONFIRMED=1` 时视为已确认(存量脚本/CI 迁移期用)。
    pub fn from_flags(dry_run: bool, confirm: bool) -> Self {
        let allow_unconfirmed = std::env::var("ATLASSIAN_CLI_ALLOW_UNCONFIRMED")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        Self {
            dry_run,
            confirm: confirm || allow_unconfirmed,
        }
    }
}

/// 写操作确认门禁:未显式确认则拒绝执行(报 PARAM_INVALID, exit 2)。
pub fn require_confirmed(policy: &WritePolicy) -> Result<(), AppError> {
    if policy.confirm {
        Ok(())
    } else {
        let suggested_cmd = build_suggested_confirm_command();
        Err(AppError::param_invalid("写操作需要显式确认")
            .with_detail("确认执行请追加 --confirm;仅预览请追加 --dry-run")
            .with_suggestion("确认执行请追加 --confirm;仅预览请追加 --dry-run")
            .with_suggested_command(suggested_cmd))
    }
}

/// 重建包含 --confirm 的完整执行命令行 (供 AI Agent 0 思考成本直接调用)
fn build_suggested_confirm_command() -> String {
    let args: Vec<String> = std::env::args().collect();
    let bin_name = args
        .first()
        .and_then(|p| std::path::Path::new(p).file_name()?.to_str())
        .unwrap_or("atlassian-cli");

    let mut clean_args = Vec::new();
    for a in args.iter().skip(1) {
        if a == "--dry-run" {
            continue;
        }
        if a.contains(' ') || a.contains('"') || a.is_empty() {
            clean_args.push(format!("\"{}\"", a.replace('"', "\\\"")));
        } else {
            clean_args.push(a.clone());
        }
    }
    clean_args.push("--confirm".to_string());
    format!("{} {}", bin_name, clean_args.join(" "))
}

/// 构造统一的写操作 dry-run 预览 JSON(零副作用)。
pub fn preview_json(
    action: &str,
    method: &str,
    path: &str,
    target: &str,
    body: Option<&Value>,
    hint: Option<&str>,
) -> Value {
    let mut v = json!({
        "status": "dry_run",
        "action": action,
        "method": method,
        "path": path,
        "target": target,
    });
    if let Some(b) = body {
        v["body"] = b.clone();
    }
    v["hint"] = json!(hint.unwrap_or("只读预览,未真正执行。确认执行请追加 --confirm"));
    v
}

/// 判断 HttpClient 返回的是否为幂等回放响应(HttpClient 层拦截命中时返回)。
/// 写操作在构造成功响应前调用:若为回放,直接透传,避免吞掉状态标记。
pub fn is_replayed(raw: &Value) -> bool {
    raw.get("status")
        .map(|s| s.as_str() == Some("idempotent_replay"))
        .unwrap_or(false)
}

/// 所有 Atlassian 产品模块的统一契约。
///
/// 一个模块 = 一个产品(Jira / Confluence / Bitbucket / 未来的 Bamboo / StatusPage…),
/// 模块内部自包含: CLI action 枚举 + API 方法 + 本 trait 的 dispatch。
pub trait AtlassianModule: Sized {
    /// 该模块的 CLI 子命令枚举(clap `Subcommand`)
    type Action;

    /// 模块名,用于错误信息前缀(如 "jira")
    fn module_name() -> &'static str;

    /// 建立连接:校验配置 + 构造带 Bearer Token 的 HTTP 客户端
    fn connect(cfg: &Config, policy: WritePolicy) -> Result<Self, AppError>;

    /// 统一的命令分发入口:把 CLI action 转成具体的 API 调用
    fn handle(
        &self,
        action: Self::Action,
    ) -> impl std::future::Future<Output = Result<Value, AppError>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;

    /// 串行化涉及环境变量的测试,避免并行竞态
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn test_write_policy_from_flags() {
        let _g = ENV_LOCK.lock().unwrap();
        let p = WritePolicy::from_flags(true, false);
        assert!(p.dry_run);
        assert!(!p.confirm);

        let p = WritePolicy::from_flags(false, true);
        assert!(!p.dry_run);
        assert!(p.confirm);
    }

    #[test]
    fn test_write_policy_env_escape_hatch() {
        let _g = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("ATLASSIAN_CLI_ALLOW_UNCONFIRMED", "1");
        }
        let p = WritePolicy::from_flags(false, false);
        assert!(p.confirm);
        unsafe {
            std::env::remove_var("ATLASSIAN_CLI_ALLOW_UNCONFIRMED");
        }
        let p2 = WritePolicy::from_flags(false, false);
        assert!(!p2.confirm);
    }

    #[test]
    fn test_require_confirmed() {
        let ok = WritePolicy {
            dry_run: false,
            confirm: true,
        };
        assert!(require_confirmed(&ok).is_ok());

        let denied = WritePolicy {
            dry_run: false,
            confirm: false,
        };
        let e = require_confirmed(&denied).unwrap_err();
        assert_eq!(e.code, ErrorCode::ParamInvalid);
        assert_eq!(e.code.exit_code(), 2);
        assert!(e.detail.as_deref().unwrap().contains("--confirm"));
        assert!(e.suggested_command.as_deref().unwrap().contains("--confirm"));
    }

    #[test]
    fn test_preview_json_structure() {
        let body = json!({ "text": "hello" });
        let v = preview_json(
            "jira.comment",
            "POST",
            "/rest/api/2/issue/PROJ-1/comment",
            "PROJ-1",
            Some(&body),
            None,
        );
        assert_eq!(v["status"], "dry_run");
        assert_eq!(v["action"], "jira.comment");
        assert_eq!(v["method"], "POST");
        assert_eq!(v["path"], "/rest/api/2/issue/PROJ-1/comment");
        assert_eq!(v["target"], "PROJ-1");
        assert_eq!(v["body"], body);
        assert!(v["hint"].as_str().unwrap().contains("--confirm"));

        let v2 = preview_json("x", "DELETE", "/p", "t", None, Some("custom hint"));
        assert!(v2.get("body").is_none());
        assert_eq!(v2["hint"], "custom hint");
    }
}
