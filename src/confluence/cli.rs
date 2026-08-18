use clap::{Args, Subcommand};

#[derive(Args)]
pub struct CreatePageArgs {
    /// Confluence Space Key (例如 PROJ 或 LLSQAG)
    #[arg(long, short = 's', alias = "space-key")]
    pub space: String,
    /// 页面标题 (Title)
    #[arg(long, short = 't', alias = "summary", alias = "name")]
    pub title: String,
    /// 页面正文内容 (支持纯文本/HTML/Markdown 及时间宏 `<time datetime="..."/>` 与 Jira 动态卡片宏)
    #[arg(long, short = 'b', alias = "text", alias = "content")]
    pub body: String,
    /// 父页面 ID (可选，指定则在此父页面下新建)
    #[arg(long, alias = "parent")]
    pub parent_id: Option<String>,
}

#[derive(Args)]
pub struct UpdatePageArgs {
    /// 页面 ID 或网页 URL (例如 224527717 或网页链接)
    pub id_or_url: String,
    /// 新的页面标题 (可选，不提供则保留原标题)
    #[arg(long, short = 't', alias = "summary", alias = "name")]
    pub title: Option<String>,
    /// 新的全量页面正文 (可选，全量覆盖)
    #[arg(long, short = 'b', alias = "text", alias = "content")]
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
}

/// Confluence 模块的 CLI 子命令
#[derive(Subcommand)]
pub enum ConfluenceActions {
    /// 搜索页面 (支持全文检索或仅按标题精准搜索 --title-only)
    Search {
        query: String,
        /// 仅按页面标题搜索 (默认全文检索)
        #[arg(long, short = 't')]
        title_only: bool,
        /// 按 Confluence 空间 (Space Key) 过滤 (例如 PROJ 或 LLSQAG)
        #[arg(long, short = 's')]
        space: Option<String>,
        /// 分页起始偏移量 (用于搜索多页结果，默认 0)
        #[arg(long, default_value_t = 0)]
        start_at: u32,
        /// 返回条数上限
        #[arg(long, default_value_t = 10)]
        limit: u32,
    },
    /// 获取页面内容 (默认转纯文本，支持 --title-only 仅查看元信息与标题，--raw 输出原始 HTML)
    Get {
        /// 页面 ID 或网页 URL
        id: String,
        /// 仅获取页面标题与基础元信息 (不拉取任何正文，省流量省 Token)
        #[arg(long, short = 't')]
        title_only: bool,
        /// 输出原始 Confluence Storage HTML (默认转换为清洗后的纯文本)
        #[arg(long)]
        raw: bool,
        /// 最大输出字符数 (默认 8000，设为 0 表示不限制)
        #[arg(long, default_value_t = 8000)]
        max_chars: usize,
        /// 字符起始偏移量 (用于续读超长文档)
        #[arg(long, default_value_t = 0)]
        offset: usize,
    },
    /// 查看某个页面的直接子页面目录清单 (仅列出子页面 ID、标题与版本，0 正文轻量化查看)
    Children {
        /// 父页面 ID 或网页 URL
        id: String,
        /// 返回条数上限 (默认 50)
        #[arg(long, default_value_t = 50)]
        limit: u32,
    },
    /// 列出或搜索当前用户有权限访问的 Confluence 空间 (Spaces 列表与 Key 查询)
    Spaces {
        /// 空间名称或 Space Key 关键字搜索过滤 (可选)
        query: Option<String>,
        /// 最多返回条数 (默认 25)
        #[arg(long, default_value_t = 25)]
        limit: u32,
    },
    /// 查看 Confluence 页面挂载的全部附件列表与下载链接
    Attachments {
        /// 页面 ID 或网页 URL
        id: String,
        /// 最多返回条数 (默认 50)
        #[arg(long, default_value_t = 50)]
        limit: u32,
    },
    /// 上传本地文件到指定 Confluence 页面作为附件
    Attach {
        /// 页面 ID 或网页 URL
        id: String,
        /// 本地文件路径 (如 ./spec.pdf 或 /path/to/image.png)
        file: String,
        /// 附件备注说明 (可选)
        #[arg(long)]
        comment: Option<String>,
    },
    /// 带 PAT 认证下载 Confluence 页面附件并保存至本地文件
    AttachmentDownload {
        /// 页面 ID 或网页 URL
        id: String,
        /// 目标附件 ID 或附件文件名 (如 "att12345" 或 "spec.pdf")
        attachment: String,
        /// 本地保存路径 (可选，默认保存在当前目录下的原始文件名)
        #[arg(long, short = 'o')]
        output: Option<String>,
    },
    /// 创建新 Confluence 页面 (原生支持时间宏与 Jira 卡片宏)
    Create(CreatePageArgs),
    /// 安全更新 Confluence 页面 (支持局部精准替换 --find / --replace、追加 --append、置顶 --prepend 与 --dry-run 预览)
    Update(UpdatePageArgs),
}
