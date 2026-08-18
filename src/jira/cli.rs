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
    #[arg(long, short = 'p', alias = "proj")]
    pub project: String,
    /// 单子标题/概要 (Summary)
    #[arg(long, short = 's', alias = "title", allow_hyphen_values = true)]
    pub summary: String,
    /// 单子类型 (默认 Task，可选 Bug / Story / Task 等)
    #[arg(long, short = 't', alias = "type", default_value = "Task")]
    pub issue_type: String,
    /// 单子详细描述 (Description)
    #[arg(long, short = 'd', alias = "desc", alias = "body", allow_hyphen_values = true)]
    pub description: Option<String>,
    /// 标签列表 (英文逗号分隔，如 "bug,backend")
    #[arg(long, short = 'l', alias = "label")]
    pub labels: Option<String>,
    /// 指派人用户名 (Assignee username)
    #[arg(long, short = 'a', alias = "user")]
    pub assignee: Option<String>,
    /// 优先级 (Priority，如 High / Medium / Low)
    #[arg(long, alias = "prio")]
    pub priority: Option<String>,
    /// 自定义字段赋值 (支持多次传入,如 --custom "customfield_10020=5" --custom "customfield_10010=PROJ-10")
    #[arg(long, value_name = "KEY=VAL")]
    pub custom: Vec<String>,
    /// 原始自定义字段 JSON 对象 (如 '{"customfield_10020": ["sprint-1"]}')
    #[arg(long, value_name = "JSON_OBJECT")]
    pub custom_json: Option<String>,
}

#[derive(Args)]
pub struct UpdateIssueArgs {
    /// 单子 Key 或网页 URL (如 PROJSA-123 或网页链接)
    pub key_or_url: String,
    /// 新的单子标题/概要 (Summary)
    #[arg(long, short = 's', alias = "title", allow_hyphen_values = true)]
    pub summary: Option<String>,
    /// 新的单子详细描述 (Description)
    #[arg(long, short = 'd', alias = "desc", alias = "body", allow_hyphen_values = true)]
    pub description: Option<String>,
    /// 指派人用户名 (Assignee username)
    #[arg(long, short = 'a', alias = "user")]
    pub assignee: Option<String>,
    /// 优先级 (Priority，如 High / Medium / Low)
    #[arg(long, alias = "prio")]
    pub priority: Option<String>,
    /// 标签列表 (英文逗号分隔，如 "bug,backend")
    #[arg(long, short = 'l', alias = "label")]
    pub labels: Option<String>,
    /// 自定义字段赋值 (支持多次传入,如 --custom "customfield_10020=5" --custom "customfield_10010=PROJ-10")
    #[arg(long, value_name = "KEY=VAL")]
    pub custom: Vec<String>,
    /// 原始自定义字段 JSON 对象 (如 '{"customfield_10020": ["sprint-1"]}')
    #[arg(long, value_name = "JSON_OBJECT")]
    pub custom_json: Option<String>,
}

#[derive(Args)]
pub struct CommentArgs {
    /// 单子 Key 或网页 URL (如 PROJSA-123 或网页链接)
    pub key: String,
    /// 评论正文 (位置参数，与 --body / --text 互为等价输入)
    #[arg(value_name = "BODY", allow_hyphen_values = true)]
    pub body_pos: Option<String>,
    /// 评论正文 (命名参数，别名 --text, --comment, -b)
    #[arg(long, short = 'b', alias = "text", alias = "comment", allow_hyphen_values = true)]
    pub body: Option<String>,
}

impl CommentArgs {
    pub fn get_text(&self) -> Result<&str, crate::error::AppError> {
        self.body
            .as_deref()
            .or(self.body_pos.as_deref())
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| crate::error::AppError::param_invalid("缺少评论正文内容 (请通过位置参数或 --body / --text 传入)"))
    }
}

#[derive(Args)]
pub struct CommentUpdateArgs {
    /// 单子 Key 或网页 URL
    pub key: String,
    /// 评论 ID (jira get 返回的 comments[].id)
    pub comment_id: String,
    /// 新的评论正文 (位置参数，与 --body / --text 互为等价输入)
    #[arg(value_name = "BODY", allow_hyphen_values = true)]
    pub body_pos: Option<String>,
    /// 新的评论正文 (命名参数，别名 --text, --comment, -b)
    #[arg(long, short = 'b', alias = "text", alias = "comment", allow_hyphen_values = true)]
    pub body: Option<String>,
}

impl CommentUpdateArgs {
    pub fn get_text(&self) -> Result<&str, crate::error::AppError> {
        self.body
            .as_deref()
            .or(self.body_pos.as_deref())
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| crate::error::AppError::param_invalid("缺少评论正文内容 (请通过位置参数或 --body / --text 传入)"))
    }
}

#[derive(Args)]
pub struct AssignArgs {
    /// 单子 Key 或网页 URL
    pub key: String,
    /// 经办人用户名 (位置参数)
    #[arg(value_name = "ASSIGNEE")]
    pub assignee_pos: Option<String>,
    /// 经办人用户名 (命名参数，别名 --user, -a, --assignee)
    #[arg(long, short = 'a', alias = "user", alias = "assignee")]
    pub user: Option<String>,
}

impl AssignArgs {
    pub fn get_assignee(&self) -> Result<&str, crate::error::AppError> {
        self.user
            .as_deref()
            .or(self.assignee_pos.as_deref())
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| crate::error::AppError::param_invalid("缺少经办人用户名 (请通过位置参数或 --user / --assignee 传入)"))
    }
}

/// Jira 模块的 CLI 子命令
#[derive(Subcommand)]
pub enum JiraActions {
    /// 查询单子详情 (支持 Key 或网页 URL，支持 --raw 原始全量输出与 --fields 指定字段)
    Get(GetIssueArgs),
    /// 在单子里加评论 (支持位置参数与 --body / --text)
    Comment(CommentArgs),
    /// 编辑单子上的已有评论 (需先通过 jira get 获取 comment_id，支持位置参数与 --body / --text)
    CommentUpdate(CommentUpdateArgs),
    /// 删除单子上的评论 (需先通过 jira get 获取 comment_id)
    CommentDelete {
        /// 单子 Key 或网页 URL
        key: String,
        /// 评论 ID (jira get 返回的 comments[].id)
        comment_id: String,
    },
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
    /// 快捷指派/变更经办人 (支持 Key 或网页 URL，支持位置参数与 --user / --assignee)
    Assign(AssignArgs),
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
    /// 带 PAT 认证下载 Jira 工单附件并保存至本地文件
    AttachmentDownload {
        /// 单子 Key 或网页 URL
        key: String,
        /// 目标附件 ID 或附件文件名 (如 "12345" 或 "crash.log")
        attachment: String,
        /// 本地保存路径 (可选，默认保存在当前目录下的原始文件名)
        #[arg(long, short = 'o')]
        output: Option<String>,
    },
    /// 查询 Jira 系统标准字段与自定义字段元数据 (将 customfield_xxx 翻译为人类可读名称)
    Fields {
        /// 字段 ID 或字段名称搜索过滤 (如 "Sprint" 或 "customfield_10020")
        #[arg(long, short = 'q')]
        query: Option<String>,
        /// 仅列出企业自定义字段 (customfield)
        #[arg(long)]
        custom_only: bool,
        /// 最多返回条数 (默认 50)
        #[arg(long, default_value_t = 50)]
        limit: u32,
    },
    /// 列出所有可见项目 (含 key/名称/类型)
    Projects {
        /// 名称关键字过滤 (可选)
        #[arg(long, short = 'q')]
        query: Option<String>,
    },
    /// 查询项目的创建元数据 (可用 issue 类型/子任务类型,避免猜类型名)
    IssueTypes {
        /// 项目 Key (可选,不传聚合所有可见项目的类型)
        #[arg(long)]
        project: Option<String>,
        /// 最多返回条数 (默认 50)
        #[arg(long, default_value_t = 50)]
        limit: u32,
    },
    /// 查看/添加/移除单子关注人 (不传 --add/--remove 时为查询)
    Watchers {
        /// 单子 Key 或网页 URL
        key: String,
        /// 添加关注人用户名
        #[arg(long)]
        add: Option<String>,
        /// 移除关注人用户名
        #[arg(long)]
        remove: Option<String>,
    },
    /// 删除单子上的附件 (支持 ID 或文件名)
    AttachmentDelete {
        /// 单子 Key 或网页 URL
        key: String,
        /// 附件 ID 或文件名 (如 "12345" 或 "crash.log")
        attachment: String,
    },
    /// 批量创建单子 (一次请求创建多个,共享项目/类型/优先级等模板字段;官方 POST /issue/bulk)
    BulkCreate(BulkCreateArgs),
    /// 克隆单子到项目 (复制业务字段、重置状态/经办人,可选 Cloners 关联与原单留痕)
    Clone(CloneArgs),
}

#[derive(Args)]
pub struct BulkCreateArgs {
    /// Jira 项目 Key (如 PROJ 或 PROJSA)
    #[arg(long)]
    pub project: String,
    /// 多个单子标题 (英文逗号分隔,如 "task A,task B,task C")
    #[arg(long)]
    pub summaries: Option<String>,
    /// 从文件读取单子标题 (每行一个,可与 --summaries 合并)
    #[arg(long)]
    pub from_file: Option<String>,
    /// 单子类型 (默认 Task)
    #[arg(long, default_value = "Task")]
    pub issue_type: String,
    /// 单子详细描述 (共享给所有批量单子)
    #[arg(long)]
    pub description: Option<String>,
    /// 标签列表 (英文逗号分隔,共享)
    #[arg(long)]
    pub labels: Option<String>,
    /// 指派人用户名 (共享)
    #[arg(long)]
    pub assignee: Option<String>,
    /// 优先级 (共享)
    #[arg(long)]
    pub priority: Option<String>,
    /// 自定义字段赋值 (支持多次传入,共享)
    #[arg(long, value_name = "KEY=VAL")]
    pub custom: Vec<String>,
    /// 原始自定义字段 JSON 对象 (共享)
    #[arg(long, value_name = "JSON_OBJECT")]
    pub custom_json: Option<String>,
}

#[derive(Args)]
pub struct CloneArgs {
    /// 源单子 Key 或网页 URL (如 PROJSA-123)
    pub source: String,
    /// 目标项目 Key (默认沿用源单项目)
    #[arg(long)]
    pub project: Option<String>,
    /// 新单子标题 (默认沿用源标题;建议加 CLONE 前缀便于区分)
    #[arg(long)]
    pub summary: Option<String>,
    /// 复制核心字段:summary/description/issuetype/priority/labels/components/fixVersions/duedate/environment (默认)
    #[arg(long)]
    pub core_only: bool,
    /// 追加复制的自定义字段 (逗号分隔,如 "customfield_10020,customfield_10010")
    #[arg(long)]
    pub extra_fields: Option<String>,
    /// 创建 Cloners 关联 (新单 ↔ 源单)
    #[arg(long)]
    pub link: bool,
    /// 在源单上追加留痕评论 (说明被克隆到哪个新单)
    #[arg(long)]
    pub comment: bool,
    /// 复制附件 (默认不复制)
    #[arg(long)]
    pub include_attachments: bool,
}

#[derive(Args)]
pub struct AddWorklogArgs {
    /// 单子 Key 或网页 URL (如 PROJSA-123 或网页链接)
    pub key_or_url: String,
    /// 消耗工时时间 (位置参数)
    #[arg(value_name = "TIME_SPENT")]
    pub time_pos: Option<String>,
    /// 消耗工时时间 (命名参数，别名 --time, --spent, -t)
    #[arg(long, short = 't', alias = "time", alias = "spent")]
    pub time_spent: Option<String>,
    /// 工时日志备注说明 (可选，别名 --comment, -c, -m, --body, --text, --desc)
    #[arg(long, short = 'm', short_alias = 'c', alias = "comment", alias = "body", alias = "text", alias = "desc", allow_hyphen_values = true)]
    pub comment: Option<String>,
    /// 开始时间 (可选，格式 "YYYY-MM-DD" 或 "YYYY-MM-DDTHH:MM:SS"，不传默认为当前时间)
    #[arg(long, short = 's', alias = "date", alias = "start")]
    pub started: Option<String>,
}

impl AddWorklogArgs {
    pub fn get_time_spent(&self) -> Result<&str, crate::error::AppError> {
        self.time_spent
            .as_deref()
            .or(self.time_pos.as_deref())
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| crate::error::AppError::param_invalid("缺少工时参数 (请通过位置参数或 --time-spent / --time 传入，如 '2h 30m')"))
    }
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
