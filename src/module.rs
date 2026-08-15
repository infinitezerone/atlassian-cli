use serde_json::Value;

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

    #[test]
    fn test_write_policy_from_flags() {
        let p = WritePolicy::from_flags(true, false);
        assert!(p.dry_run);
        assert!(!p.confirm);

        let p = WritePolicy::from_flags(false, true);
        assert!(!p.dry_run);
        assert!(p.confirm);
    }

    #[test]
    fn test_write_policy_env_escape_hatch() {
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
}
