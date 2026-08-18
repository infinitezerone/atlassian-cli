use clap::{Args, Subcommand};

#[derive(Args)]
pub struct ListPrsArgs {
    /// Bitbucket Project 名 (若提供 --url 则可省略)
    #[arg(long)]
    pub project: Option<String>,
    /// Bitbucket Repo 名 (若提供 --url 则可省略)
    #[arg(long)]
    pub repo: Option<String>,
    /// 仓库网页 URL (例如 https://bitbucket.example.com/projects/PROJ/repos/my-repo)
    #[arg(long)]
    pub url: Option<String>,
    /// PR 状态 (默认 OPEN，可选 OPEN / MERGED / DECLINED / ALL)
    #[arg(long, default_value = "OPEN")]
    pub state: String,
    /// 最多返回条数 (默认 10)
    #[arg(long, default_value_t = 10)]
    pub limit: u32,
}

#[derive(Args)]
pub struct CommentPrArgs {
    /// Bitbucket Project 名 (若传入完整 PR 网页 URL 则自动从 URL 解析)
    #[arg(long, short = 'p', alias = "proj")]
    pub project: Option<String>,
    /// Bitbucket Repo 名 (若传入完整 PR 网页 URL 则自动从 URL 解析)
    #[arg(long, short = 'r', alias = "repository")]
    pub repo: Option<String>,
    /// PR ID 或完整 PR 网页 URL (例如 2420 或网页链接)
    pub id_or_url: String,
    /// 评论文本内容 (位置参数)
    #[arg(value_name = "BODY")]
    pub body_pos: Option<String>,
    /// 评论文本内容 (命名参数，别名 --body, -b, --text, --comment)
    #[arg(long, short = 'b', alias = "text", alias = "comment")]
    pub body: Option<String>,
    /// 行内评论的目标文件相对路径 (如 src/main.rs，不指定则为 PR 全局评论)
    #[arg(long, short = 'f', alias = "path")]
    pub file: Option<String>,
    /// 行内评论的目标代码行号 (如 42)
    #[arg(long, short = 'l')]
    pub line: Option<u32>,
    /// 目标代码行的 Diff 类型 (默认 ADDED，可选 ADDED / REMOVED / CONTEXT)
    #[arg(long, default_value = "ADDED")]
    pub line_type: String,
    /// 目标文件视角 (默认 TO 表示修改后的目标文件，FROM 表示修改前)
    #[arg(long, default_value = "TO")]
    pub file_type: String,
}

impl CommentPrArgs {
    pub fn get_text(&self) -> Result<&str, crate::error::AppError> {
        self.body
            .as_deref()
            .or(self.body_pos.as_deref())
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| crate::error::AppError::param_invalid("缺少 PR 评论正文内容 (请通过位置参数或 --body / --text 传入)"))
    }
}

#[derive(Args)]
pub struct CreatePrArgs {
    /// Bitbucket Project 名 (例如 PROJ)
    #[arg(long, short = 'p', alias = "proj")]
    pub project: String,
    /// Bitbucket Repo 名 (例如 my-repo)
    #[arg(long, short = 'r', alias = "repository")]
    pub repo: String,
    /// PR 标题/概要 (Summary)
    #[arg(long, short = 't', alias = "summary")]
    pub title: String,
    /// PR 详细描述 (Description)
    #[arg(long, short = 'd', alias = "desc", alias = "body", default_value = "")]
    pub description: String,
    /// 源分支名 (如 feature/add-login)
    #[arg(long, short = 'f', alias = "source", alias = "src", alias = "from-branch")]
    pub from: String,
    /// 目标分支名 (如 main 或 release/6.2.0)
    #[arg(long, alias = "target", alias = "dest", alias = "to-branch")]
    pub to: String,
    /// 手动指定的额外 Reviewer 用户名列表 (英文逗号分隔，如 "john.doe, jane.smith" 或 @{john.doe})
    #[arg(long, alias = "reviewer")]
    pub reviewers: Option<String>,
    /// 不自动加载 Bitbucket 网页端预设的 Default Reviewers (默认 false，即自动包含预设)
    #[arg(long, default_value_t = false)]
    pub no_default_reviewers: bool,
}

#[derive(Args)]
pub struct GetPrArgs {
    /// Bitbucket Project 名 (例如 PROJ，若传入完整 PR 网页 URL 则自动从 URL 解析)
    #[arg(long)]
    pub project: Option<String>,
    /// Bitbucket Repo 名 (例如 my-repo，若传入完整 PR 网页 URL 则自动从 URL 解析)
    #[arg(long)]
    pub repo: Option<String>,
    /// PR ID 或完整 PR 网页 URL (例如 2420 或 https://gitpub.../pull-requests/2420/overview)
    pub id_or_url: String,
}

#[derive(Args)]
pub struct DiffPrArgs {
    /// Bitbucket Project 名 (例如 PROJ，若传入完整 PR 网页 URL 则自动从 URL 解析)
    #[arg(long)]
    pub project: Option<String>,
    /// Bitbucket Repo 名 (例如 my-repo，若传入完整 PR 网页 URL 则自动从 URL 解析)
    #[arg(long)]
    pub repo: Option<String>,
    /// PR ID 或完整 PR 网页 URL (例如 2420 或 https://gitpub.../pull-requests/2420/overview)
    pub id_or_url: String,
    /// 仅返回变更文件路径与增减行统计大纲 (--stat)，不输出庞大 Diff 正文 (极度省 Token)
    #[arg(long, short = 's')]
    pub stat: bool,
    /// 仅提取并查看指定单文件的 Diff 内容 (如 src/main.rs)
    #[arg(long, short = 'f')]
    pub file: Option<String>,
    /// 最大输出 Diff 行数限制 (默认 1000，设为 0 表示不限制)
    #[arg(long, default_value_t = 1000)]
    pub max_lines: usize,
    /// 行起始偏移量 (用于超长 Diff 续读)
    #[arg(long, default_value_t = 0)]
    pub offset: usize,
}

/// Bitbucket 模块的 CLI 子命令
#[derive(Subcommand)]
pub enum BitbucketActions {
    /// 查询 Pull Request 列表 (支持 --project --repo 或直接传入仓库网页 URL)
    ListPrs(ListPrsArgs),
    /// 创建 Pull Request
    CreatePr(CreatePrArgs),
    /// 获取 PR 详情 (支持直接传入网页 URL)
    GetPr(GetPrArgs),
    /// 查看 PR 代码修改差异与变动文件 (支持 --stat 概览、--file 单文件查看与 --max-lines 截断)
    DiffPr(DiffPrArgs),
    /// 查看 PR 的评论讨论树与活动记录 (支持直接传入网页 URL)
    CommentsPr(GetPrArgs),
    /// 在 PR 上发表评论 (支持直接传入网页 URL)
    CommentPr(CommentPrArgs),
    /// 批准/同意 PR (Approve Pull Request，支持直接传入网页 URL)
    ApprovePr(GetPrArgs),
    /// 按姓名或邮箱模糊搜索同事 (返回 displayName, email 与防误触 @ 语法 mention_syntax)
    User {
        /// 姓名或邮箱关键字 (如 "John" 或 "john.doe@...")
        query: String,
        /// 最多返回条数 (默认 10)
        #[arg(long, default_value_t = 10)]
        limit: u32,
    },
}
