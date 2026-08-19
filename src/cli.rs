use clap::{Parser, Subcommand};

use crate::bitbucket::BitbucketActions;
use crate::confluence::ConfluenceActions;
use crate::jira::JiraActions;

#[derive(Parser)]
#[command(
    name = "atlassian-cli",
    about = "Atlassian 私有部署统一 AI CLI (Jira + Confluence + Bitbucket)",
    version
)]
pub struct Cli {
    /// 允许自签名 / 无效 TLS 证书 (不校验 HTTPS 证书合法性)
    #[arg(long, short = 'k', global = true)]
    pub insecure: bool,

    /// 写操作仅预览 (打印将执行的请求,不真正调用 API)
    #[arg(long, global = true)]
    pub dry_run: bool,

    /// 显式确认写操作 (不传则写操作将被拒绝执行)
    #[arg(long, global = true)]
    pub confirm: bool,

    /// 格式化 Pretty-Printed JSON 输出 (带缩进换行;默认紧凑单行 JSON,最大化节省 Token)
    #[arg(long, global = true)]
    pub pretty: bool,

    #[command(subcommand)]
    pub command: Commands,
}

/// 顶层命令 = 产品清单 + 快捷接入命令
#[derive(Subcommand)]
pub enum Commands {
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
        action: JiraActions,
    },
    /// Confluence 操作
    Confluence {
        #[command(subcommand)]
        action: ConfluenceActions,
    },
    /// Bitbucket 操作
    Bitbucket {
        #[command(subcommand)]
        action: BitbucketActions,
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
    /// 查看本地写操作审计日志 (谁在何时改了什么,含幂等回放标记)
    Audit {
        /// 最多返回条数 (默认 20)
        #[arg(long, default_value_t = 20)]
        limit: u32,
    },
    /// 检查是否有新版本发布 (对比 GitHub 最新 Release)
    CheckUpdate,
}

#[derive(Subcommand)]
pub enum SkillSubActions {
    /// 自动安装/更新官方 Agent Skill 到全局配置目录 (~/.gemini/config/skills/atlassian-cli/SKILL.md)
    Install,
    /// 检查 Agent Skill 的部署状态与文件路径
    Status,
    /// 从所有常见 Agent 目录卸载本 Skill (删除整个 skill 目录,含 references/)
    Uninstall,
}

#[derive(Subcommand)]
pub enum ConfigActions {
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
