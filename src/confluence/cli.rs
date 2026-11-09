use clap::{Args, Subcommand};

#[derive(Args)]
pub struct CreatePageArgs {
    /// Confluence Space Key (例如 SPACE 或 ENG)
    #[arg(long, short = 's', alias = "space-key")]
    pub space: String,
    /// 页面标题 (Title)
    #[arg(long, short = 't', alias = "summary", alias = "name", allow_hyphen_values = true)]
    pub title: String,
    /// 页面正文内容 (支持纯文本/HTML/Markdown 及时间宏 `<time datetime="..."/>` 与 Jira 动态卡片宏)
    #[arg(long, short = 'b', alias = "text", alias = "content", allow_hyphen_values = true)]
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
    #[arg(long, short = 't', alias = "summary", alias = "name", allow_hyphen_values = true)]
    pub title: Option<String>,
    /// 新的全量页面正文 (可选，全量覆盖)
    #[arg(long, short = 'b', alias = "text", alias = "content", allow_hyphen_values = true)]
    pub body: Option<String>,
    /// 局部原子替换：需寻找的目标旧文本 (需包含唯一上下文段落，与 --replace 配对)
    #[arg(long, allow_hyphen_values = true)]
    pub find: Option<String>,
    /// 局部原子替换：替换后的新文本 (与 --find 配对)
    #[arg(long, allow_hyphen_values = true)]
    pub replace: Option<String>,
    /// 在页面最末尾追加的内容 (不改变原页面任何旧排版与宏)
    #[arg(long, allow_hyphen_values = true)]
    pub append: Option<String>,
    /// 在页面最顶端插入的内容 (不改变原页面任何旧排版与宏)
    #[arg(long, allow_hyphen_values = true)]
    pub prepend: Option<String>,
    /// 表格单元格原子更新：三元组参数 <ROW_ANCHOR> <COL> <VALUE>
    /// 支持按列名定位 (如 "状态") 或按 1-based 列序号定位 (如 "3")，值支持宏简写如 [Status: DONE]、[~user]
    /// 示例: --set-cell "支付网关" "状态" "DONE"
    #[arg(long, num_args = 3, value_names = ["ROW_ANCHOR", "COL", "VALUE"], allow_hyphen_values = true)]
    pub set_cell: Option<Vec<String>>,
    /// 表格末尾原子增行：多元组参数 <TABLE_ANCHOR> <CELL_VALUES...>
    /// 示例: --append-row "发版清单" "搜索模块" "[~zhangsan]" "[Status: IN_PROGRESS]" "2026-09-01"
    #[arg(long, num_args = 2.., value_names = ["TABLE_ANCHOR", "CELL_VALUES"], allow_hyphen_values = true)]
    pub append_row: Option<Vec<String>>,
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
        /// 按 Confluence 空间 (Space Key) 过滤 (例如 SPACE 或 ENG)
        #[arg(long, short = 's')]
        space: Option<String>,
        /// 分页起始偏移量 (用于搜索多页结果，默认 0)
        #[arg(long, default_value_t = 0)]
        start_at: u32,
        /// 返回条数上限
        #[arg(long, default_value_t = 10)]
        limit: u32,
    },
    /// 获取页面内容 (默认转结构化 Markdown 文本保留表格与排版，支持 --title-only 仅查看元信息与标题，--raw 输出原始 Storage HTML)
    Get {
        /// 页面 ID 或网页 URL
        id: String,
        /// 仅获取页面标题与基础元信息 (不拉取任何正文，省流量省 Token)
        #[arg(long, short = 't')]
        title_only: bool,
        /// 输出原始 Confluence Storage HTML (默认转换为保留表格与格式的 Markdown)
        #[arg(long)]
        raw: bool,
        /// 按小节标题语义定向抽取正文 (如 --section "Comments" 或 --section "Description")
        #[arg(long)]
        section: Option<String>,
        /// 仅定向提取指定表格 (支持 1-based 表格序号如 --table 1，或表头/内容关键字如 --table "状态")
        #[arg(long)]
        table: Option<String>,
        /// 最大输出字符数 (默认 32000，设为 0 表示不限制；建议大文档单次拉取 32000~64000)
        #[arg(long, default_value_t = 32000)]
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
