use clap::Subcommand;

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
}
