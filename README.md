# atlassian-cli

Atlassian 私有部署 (Data Center / Server) 统一 CLI：**Jira + Confluence + Bitbucket** 三件套。
所有命令均输出**裁剪后的精简 JSON**，内置网络自动重试、指数退避与安全转义，方便 AI Agent 直接消费。

## 快速上手

### 1. 安装

**方式 A：Homebrew 一键安装（macOS 推荐 ⭐️）**

```bash
brew install infinitezerone/tap/atlassian-cli
```

**方式 B：一键 Shell 脚本**

```bash
curl -fsSL https://raw.githubusercontent.com/infinitezerone/atlassian-cli/main/install.sh | sh
```

**方式 C：源码编译（需要 Rust 环境）**

```bash
git clone https://github.com/infinitezerone/atlassian-cli.git
cd atlassian-cli
cargo build --release
./install.sh            # 自动探测 OS/架构,装入 PATH (/usr/local/bin 或 ~/.local/bin)
```

### 2. 配置（一次性）

```bash
atlassian-cli login        # 交互式配置 Base URL + 三个 PAT，落盘后自动测试连通性
```

也可以用环境变量：`JIRA_URL` / `JIRA_TOKEN`、`CONFLUENCE_URL` / `CONFLUENCE_TOKEN`、`BITBUCKET_URL` / `BITBUCKET_TOKEN`。

### 3. 使用

```bash
# 检查连通状态与身份
atlassian-cli status

# ---- Jira ----
atlassian-cli jira search "assignee = currentUser() AND status != Closed"  # JQL 搜单
atlassian-cli jira get PROJ-1024                                            # 查单详情 (支持 Key 或网页 URL)
atlassian-cli jira comment PROJ-1024 "已修复,请复核"                         # 添加评论
atlassian-cli jira transition PROJ-1024 "In Progress"                       # 流转单子状态
atlassian-cli jira create --project PROJ --summary "Bug标题" \               # 创建单子
  --issue-type Bug --description "..." --labels "bug,backend" --priority High

# ---- Confluence ----
atlassian-cli confluence search "支付接口说明" --limit 10                   # 全文搜索
atlassian-cli confluence get 123456                                          # 页面正文 (转纯文本, 截断 8000 字符)
atlassian-cli confluence get 123456 --max-chars 20000 --offset 8000          # 超长文档续读
atlassian-cli confluence get 123456 --raw                                    # 输出原始 HTML

# ---- Bitbucket (get/diff/comments 均支持直接粘贴网页 URL) ----
atlassian-cli bitbucket get-pr https://bitbucket.example.com/projects/PROJ/repos/repo/pull-requests/2420
atlassian-cli bitbucket diff-pr 2420 --project PROJ --repo my-repo
atlassian-cli bitbucket comments-pr 2420 --project PROJ --repo my-repo
atlassian-cli bitbucket comment-pr 2420 --project PROJ --repo my-repo --text "LGTM!"
atlassian-cli bitbucket create-pr --project P --repo R --title T --description D --from F --to T
```

## 发布新版本

发布在 **macOS 本机**完成,构建后 PUT 上传到 GitLab Generic Package Registry:

```bash
# 日常开发(不改版本号)
git commit -am "feat: ..." && git push

# 发版:构建并上传到 Package Registry(tag 版本 + latest 覆盖)
export GITLAB_TOKEN=...       # 一次性:GitLab → 头像 → Access Tokens → api scope
./scripts/publish-macos.sh v0.2.0
```

> 注意:该 GitLab 实例的 Generic Registry 上传需 `Content-Type: application/json`,
> 且 API 路径用数字项目 ID(889),脚本已处理;项目 ID 可用 `PROJECT_ID` 覆盖。

用户升级 / 回滚:

```bash
export GITLAB_TOKEN=... && ./install.sh            # latest
export GITLAB_TOKEN=... && ./install.sh -v v0.1.0  # 回滚指定版本
```

## 命令参考

| 分类 | 命令 | 说明 |
| :--- | :--- | :--- |
| **接入与状态** | `login` (或 `setup`) | 交互式配置 Base URL 与 PAT，自动发包检测连通性 |
| | `status` (或 `whoami`) | 查看配置状态、TLS 开关与已登录身份 |
| **Jira** | `jira get <KEY/URL>` | 查单详情（Key 或网页 URL 均可） |
| | `jira search <JQL>` | JQL 条件搜索 |
| | `jira comment <KEY> <TEXT>` | 添加评论 |
| | `jira transition <KEY> <STATUS>` | 流转状态（按状态名） |
| | `jira create --project P --summary S [--issue-type Bug] [--description] [--labels] [--assignee] [--priority]` | 创建单子 |
| **Confluence** | `confluence search <Q> [--limit N]` | 全文搜索 |
| | `confluence get <ID/URL> [--raw] [--max-chars N] [--offset N]` | 取正文（默认纯文本；`--max-chars 0` 不限长；`--offset` 续读） |
| **Bitbucket** | `bitbucket get-pr <URL/ID> [--project P --repo R]` | PR 概览 |
| | `bitbucket diff-pr <URL/ID> ...` | PR 代码 Diff 与变更文件 |
| | `bitbucket comments-pr <URL/ID> ...` | PR 评论讨论树 |
| | `bitbucket comment-pr <URL/ID> --text "..."` | 发表 PR 评论 |
| | `bitbucket create-pr --project P --repo R --title T --description D --from F --to T` | 创建 PR |
| **配置** | `config init` | 全量交互式初始化（落盘后自动测试连通性） |
| | `config set <module>` | 交互式设置 Token（暗显输入，推荐） |
| | `config set <module> --stdin` | 从标准输入读 Token（防 ps/history 泄漏，推荐脚本） |
| | `config set <module> <TOKEN>` | 命令行直接指定（不推荐） |
| | `config set-url <module> <URL>` | 单独改 Base URL |
| | `config unset <module>` | 清除模块配置与凭据 |
| | `config status` | 查看配置状态（Token 打码） |
| | `config test` | 测试连通性与 PAT 有效性 |
| | `config path` | 打印配置文件路径 |

> 模块名：`jira` / `confluence` / `bitbucket`。
> `--insecure`（或 `-k`）为全局标志，可加在任何子命令前，用于内网自签名证书环境。

## 配置与安全规则

- **配置文件**：`~/.atlassian-cli/config.json`（权限：目录 `0700`，文件 `0600`，仅当前用户可读写）
- **优先级**：`全局标志 (--insecure)` > `环境变量` > `config.json` > `默认值`
- **Token 安全**：
  - 优先在本机终端用 `config set <module>`（暗显）或 `echo "PAT" | config set <module> --stdin` 输入，避免出现在进程列表 / shell history
  - 三个 PAT 相互独立，分别在 Jira / Confluence / Bitbucket 个人设置生成
  - 配置文件已被 `.gitignore` 覆盖，禁止入库
- **自签名证书**：`atlassian-cli --insecure <command>` 或环境变量 `ALLOW_INSECURE_CERTS=1`

## 如何扩展

每个产品是一个自包含模块，统一实现 `AtlassianModule` trait（`src/module.rs`）。

**给已有产品加一个 API（约 10 行）**：在对应模块的 action enum 加一个变体 → struct 加一个方法（复用 `self.http.get/post`）→ `handle()` 的 match 加一行。

**新增一个产品（如 Bamboo / StatusPage）**：新建 `src/<name>.rs`（action enum + struct + trait impl）→ `main.rs` 的 `Commands` 加变体 + 装配处加一行 `run::<Name>(&cfg, action).await`。

```
src/
├── main.rs       # 薄装配层:顶层命令 + 泛型 run() 分发
├── module.rs     # AtlassianModule trait 契约 (connect + handle)
├── config.rs     # 配置加载/安全落盘/连通性测试
├── http.rs       # 统一 HTTP 客户端 (Bearer / 重试 / 自签证书)
├── utils.rs      # URL → Key/ID/PR 解析
├── jira.rs       # Jira 模块
├── confluence.rs # Confluence 模块
└── bitbucket.rs  # Bitbucket 模块
```
