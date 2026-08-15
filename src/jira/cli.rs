use clap::{Args, Subcommand};

#[derive(Args)]
pub struct GetIssueArgs {
    /// 单子 Key 或网页 URL (如 PROJSA-123 或网页链接)
    pub key: String,
    /// 最多包含的最新评论条数 (默认 10，设为 0 可不返回评论)
    #[arg(long, default_value_t = 10)]
    pub comments_limit: u32,
    /// 返回原始未经裁剪的全量 Jira API JSON 响应 (包含所有自定义字段与 timetracking)
    #[arg(long, short = 'r')]
    pub raw: bool,
    /// 自定义额外输出的字段列表 (英文逗号分隔，如 "timetracking,worklog,components,customfield_10001")
    #[arg(long, short = 'f')]
    pub fields: Option<String>,
}

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
    /// 查询单子详情 (支持 Key 或网页 URL，支持 --raw 原始全量输出与 --fields 指定字段)
    Get(GetIssueArgs),
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
        /// 逗号分隔的返回字段列表 (如 summary,status,assignee)，省 Token；支持 - 前缀排除
        #[arg(long)]
        fields: Option<String>,
        /// 分页起始索引 (0-based)，与 --limit 配合翻页
        #[arg(long, default_value_t = 0)]
        start_at: u32,
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
    /// 查询 JQL 可用字段与函数 (官方 autocompletedata API) — 拼 JQL 前先查,避免拼错字段名
    SuggestFields,
    /// JQL 字段候选值补全 (如 --field assignee --query jo) — 避免拼错用户名/项目/状态等值
    SuggestValues {
        /// 字段名 (如 assignee / reporter / project / status / fixVersion)
        field: String,
        /// 模糊搜索关键字 (可选,不传返回热门值)
        query: Option<String>,
        /// 最多返回条数 (默认 10)
        #[arg(long, default_value_t = 10)]
        limit: u32,
    },
    /// 在单子上登记工作日志与工时 (支持 "2h 30m" / "1d" / "45m", --comment, --started)
    WorklogAdd(AddWorklogArgs),
    /// 查看单子上的历史工作日志与工时登记列表
    WorklogList(ListWorklogsArgs),
    /// 删除单子上的指定工作日志工时记录 (支持 Key 或网页 URL + Worklog ID)
    WorklogDelete(DeleteWorklogArgs),
    /// 查询单子当前所有可用的流转动作与目标状态 (避免盲猜状态名)
    Transitions {
        /// 单子 Key 或网页 URL
        key: String,
    },
    /// 建立两个 Jira 单子之间的关联关系 (支持 Relates / Blocks / Cloners / Duplicate 等)
    Link {
        /// 源单子 Key 或网页 URL
        from_key: String,
        /// 目标单子 Key 或网页 URL
        to_key: String,
        /// 关联类型 (默认 Relates, 可选 Blocks, Cloners, Duplicate, Causes 等)
        #[arg(long, default_value = "Relates")]
        r#type: String,
        /// 关联备注说明 (可选)
        #[arg(long)]
        comment: Option<String>,
    },
    /// 查看单子挂载的全部附件列表与下载链接
    Attachments {
        /// 单子 Key 或网页 URL
        key: String,
    },
    /// 上传本地文件到指定 Jira 单子作为附件
    Attach {
        /// 单子 Key 或网页 URL
        key: String,
        /// 本地文件路径 (如 ./crash.log 或 /path/to/screenshot.png)
        file: String,
    },
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

#[derive(Args)]
pub struct DeleteWorklogArgs {
    /// 单子 Key 或网页 URL (如 PROJSA-123 或网页链接)
    pub key_or_url: String,
    /// 要删除的 Worklog ID (可先通过 worklog-list 获取)
    pub worklog_id: String,
}
