use anyhow::Result;
use serde_json::Value;

use crate::config::Config;

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
    fn connect(cfg: &Config) -> Result<Self>;

    /// 统一的命令分发入口:把 CLI action 转成具体的 API 调用
    fn handle(
        &self,
        action: Self::Action,
    ) -> impl std::future::Future<Output = Result<Value>> + Send;
}
