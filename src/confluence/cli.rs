use clap::{Args, Subcommand};

#[derive(Args)]
pub struct CreatePageArgs {
    /// Confluence Space Key (例如 PROJ 或 LLSQAG)
    #[arg(long)]
    pub space: String,
    /// 页面标题 (Title)
    #[arg(long)]
    pub title: String,
    /// 页面正文内容 (支持纯文本/HTML/Markdown 及时间宏 `<time datetime="..."/>` 与 Jira 动态卡片宏)
    #[arg(long)]
    pub body: String,
    /// 父页面 ID (可选，指定则在此父页面下新建)
    #[arg(long)]
    pub parent_id: Option<String>,
}

#[derive(Args)]
pub struct UpdatePageArgs {
    /// 页面 ID 或网页 URL (例如 224527717 或网页链接)
    pub id_or_url: String,
    /// 新的页面标题 (可选，不提供则保留原标题)
    #[arg(long)]
    pub title: Option<String>,
    /// 新的全量页面正文 (可选，全量覆盖)
    #[arg(long)]
    pub body: Option<String>,
    /// 局部原子替换：需寻找的目标旧文本 (需包含唯一上下文段落，与 --replace 配对)
    #[arg(long)]
    pub find: Option<String>,
    /// 局部原子替换：替换后的新文本 (与 --find 配对)
    #[arg(long)]
    pub replace: Option<String>,
    /// 在页面最末尾追加的内容 (不改变原页面任何旧排版与宏)
    #[arg(long)]
    pub append: Option<String>,
    /// 在页面最顶端插入的内容 (不改变原页面任何旧排版与宏)
    #[arg(long)]
    pub prepend: Option<String>,
    /// 只读预览 Diff，不真正提交修改到 Confluence (默认为 false)
    #[arg(long, default_value_t = false)]
    pub dry_run: bool,
}

/// Confluence 模块的 CLI 子命令
#[derive(Subcommand)]
pub enum ConfluenceActions {
    /// 全文搜索页面
    Search {
        query: String,
        /// 返回条数上限
        #[arg(long, default_value_t = 10)]
        limit: u32,
    },
    /// 获取页面正文 (默认转纯文本, --raw 输出原始 HTML)
    Get {
        id: String,
        #[arg(long)]
        raw: bool,
        /// 最大输出字符数 (默认 8000，设为 0 表示不限制)
        #[arg(long, default_value_t = 8000)]
        max_chars: usize,
        /// 字符起始偏移量 (用于续读超长文档)
        #[arg(long, default_value_t = 0)]
        offset: usize,
    },
    /// 创建新 Confluence 页面 (原生支持时间宏与 Jira 卡片宏)
    Create(CreatePageArgs),
    /// 安全更新 Confluence 页面 (支持局部精准替换 --find / --replace、追加 --append、置顶 --prepend 与 --dry-run 预览)
    Update(UpdatePageArgs),
}
