use clap::{Args, Subcommand};

#[derive(Args)]
pub struct CreateIssueArgs {
    /// Jira 项目 Key (如 PROJ 或 PROJSA)
    #[arg(long)]
    pub project: String,
    /// 单子标题/概要 (Summary)
    #[arg(long)]
    pub summary: String,
    /// 单子类型 (默认 Task，可选 Bug / Story / Task 等)
    #[arg(long, default_value = "Task")]
    pub issue_type: String,
    /// 单子详细描述 (Description)
    #[arg(long)]
    pub description: Option<String>,
    /// 标签列表 (英文逗号分隔，如 "bug,backend")
    #[arg(long)]
    pub labels: Option<String>,
    /// 指派人用户名 (Assignee username)
    #[arg(long)]
    pub assignee: Option<String>,
    /// 优先级 (Priority，如 High / Medium / Low)
    #[arg(long)]
    pub priority: Option<String>,
}

#[derive(Args)]
pub struct UpdateIssueArgs {
    /// 单子 Key 或网页 URL (如 PROJSA-123 或网页链接)
    pub key_or_url: String,
    /// 新的单子标题/概要 (Summary)
    #[arg(long)]
    pub summary: Option<String>,
    /// 新的单子详细描述 (Description)
    #[arg(long)]
    pub description: Option<String>,
    /// 指派人用户名 (Assignee username)
    #[arg(long)]
    pub assignee: Option<String>,
    /// 优先级 (Priority，如 High / Medium / Low)
    #[arg(long)]
    pub priority: Option<String>,
    /// 标签列表 (英文逗号分隔，如 "bug,backend")
    #[arg(long)]
    pub labels: Option<String>,
}

/// Jira 模块的 CLI 子命令
#[derive(Subcommand)]
pub enum JiraActions {
    /// 查询单子详情 (支持 Key 或网页 URL，包含最新评论)
    Get {
        key: String,
        /// 最多包含的最新评论条数 (默认 10，设为 0 可不返回评论)
        #[arg(long, default_value_t = 10)]
        comments_limit: u32,
    },
    /// 在单子里加评论
    Comment { key: String, text: String },
    /// 流转单子状态 (按状态名,如 In Progress / Done)
    Transition { key: String, status: String },
    /// JQL 条件搜索单子 (如 "assignee = currentUser() AND status != Closed")
    Search {
        jql: String,
        /// 最多返回条数 (默认 10)
        #[arg(long, default_value_t = 10)]
        limit: u32,
    },
    /// 创建新 Jira 单子
    Create(CreateIssueArgs),
    /// 更新已有 Jira 单子属性 (支持 Key 或网页 URL)
    Update(UpdateIssueArgs),
    /// 快捷指派/变更经办人 (支持 Key 或网页 URL)
    Assign {
        /// 单子 Key 或网页 URL
        key: String,
        /// 经办人用户名 (Assignee username)
        assignee: String,
    },
    /// 按姓名或邮箱模糊搜索同事 (返回 displayName, email 与防误触 @ 语法 mention_syntax)
    User {
        /// 姓名或邮箱关键字 (如 "John" 或 "john.doe@...")
        query: String,
        /// 最多返回条数 (默认 10)
        #[arg(long, default_value_t = 10)]
        limit: u32,
    },
    /// 查询当前单子所有合法且在职的可指派同事列表 (对应网页端 Assignee 输入框提示)
    AssignableUsers {
        /// 单子 Key 或网页 URL
        key: String,
        /// 姓名或邮箱搜索过滤 (可选)
        query: Option<String>,
        /// 最多返回条数 (默认 10)
        #[arg(long, default_value_t = 10)]
        limit: u32,
    },
    /// 在单子上登记工作日志与工时 (支持 "2h 30m" / "1d" / "45m", --comment, --started)
    WorklogAdd(AddWorklogArgs),
    /// 查看单子上的历史工作日志与工时登记列表
    WorklogList(ListWorklogsArgs),
}

#[derive(Args)]
pub struct AddWorklogArgs {
    /// 单子 Key 或网页 URL (如 PROJSA-123 或网页链接)
    pub key_or_url: String,
    /// 消耗工时时间 (支持 "2h 30m" / "1d" / "45m" 等标准格式)
    pub time_spent: String,
    /// 工时日志备注说明 (可选)
    #[arg(long, short = 'm')]
    pub comment: Option<String>,
    /// 开始时间 (可选，格式 "YYYY-MM-DD" 或 "YYYY-MM-DDTHH:MM:SS"，不传默认为当前时间)
    #[arg(long)]
    pub started: Option<String>,
}

#[derive(Args)]
pub struct ListWorklogsArgs {
    /// 单子 Key 或网页 URL (如 PROJSA-123 或网页链接)
    pub key_or_url: String,
}
