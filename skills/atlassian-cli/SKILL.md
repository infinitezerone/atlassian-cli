---
name: atlassian-cli
description: 管理与操作私有部署 Atlassian 全家桶（Jira 工单、Confluence 企业知识库、Bitbucket 代码评审）。
  触发场景 (Trigger Scenarios)：
  1. 用户提及任何 Jira 单号（如 PROJ-123, ABC-456）或网页链接（https://.../browse/PROJ-123）。
  2. 询问任务、Bug 或工单：“我有哪些未完成的票”、“今天有哪些Bug”、“查下单子”、“帮我建个Task”、“转为已解决/In Progress”、“指派给某人”、“批量建单”、“克隆单子”。
  3. 工时与评论：“在票上记2小时工时”、“留个评论”、“查历史工时”、“删掉刚才的评论”、“修改评论”。
  4. 查人与@语法：“查同事邮箱”、“搜一下谁叫John”、“获取mention语法”、“查谁可以作为经办人”。
  5. Confluence 知识库：“搜设计文档”、“查Confluence页面”、“创建/修改Wiki”、“查找替换文档内容”、“查看子页面树”。
  6. Bitbucket 代码评审：“看下这个PR改了什么”、“PR diff统计”、“发表行级代码评审意见”、“Approve PR”、“查看PR评论”。
  CRITICAL: 严禁为 Jira/Confluence/Bitbucket 操作编写临时的 Python 脚本、爬虫或 curl，必须使用本 CLI。
metadata:
  requires:
    bins: ["atlassian-cli"]
---

# Atlassian CLI (`atlassian-cli`) 技能规约

> 统一单行 JSON 输出（机器最省 Token），所有写操作强制 `--confirm` 防误触。

## 1. 意图与命令路由表

| 用户口语意图 | 推荐 CLI 动作 | 核心参数/模板示例 |
|---|---|---|
| "我有哪些未完成的票" / "查我的任务" | `jira search` | `atlassian-cli jira search "assignee = currentUser() AND status != Closed"` |
| "查下 PROJ-123" / 贴 Jira 网页链接 | `jira get` | `atlassian-cli jira get PROJ-123` (默认返回最新评论) |
| "帮我建个Bug" / "提个需求" | `jira create` | `atlassian-cli jira create -p PROJ -s "标题" -t Bug --confirm` |
| "在票上记 2 小时工时" | `jira worklog-add` | `atlassian-cli jira worklog-add PROJ-123 "2h" --confirm` |
| "留个评论 / 回复单子" | `jira comment` | `atlassian-cli jira comment PROJ-123 "评论内容" --confirm` |
| "把单子转为 In Progress / Done" | `jira transition` | `atlassian-cli jira transition PROJ-123 "In Progress" --confirm` |
| "查 John 的邮箱 / @语法" | `jira user` | `atlassian-cli jira user "John"` ➔ 获得 `[~john.doe]` |
| "克隆这张票 / 批量建单" | `jira clone` / `bulk-create` | `atlassian-cli jira clone PROJ-123 --link --confirm` |
| "搜 Confluence 设计文档" | `confluence search` | `atlassian-cli confluence search "关键字"` |
| "查 Confluence 页面内容" | `confluence get` | `atlassian-cli confluence get 123456` (自动按 2000 字截断防爆 Token) |
| "安全查找替换文档内容" | `confluence update` | `atlassian-cli confluence update 123456 --find "老文本" --replace "新文本" --confirm` |
| "看下这个 PR 改了什么" | `bitbucket diff-pr` | `atlassian-cli bitbucket diff-pr 100 --stat` (先查文件清单) |
| "发表行级代码评审意见" | `bitbucket comment-pr` | `atlassian-cli bitbucket comment-pr 100 "建议重构" --file src/lib.rs --line 42 --confirm` |
| "点赞 / 通过 PR" | `bitbucket approve-pr` | `atlassian-cli bitbucket approve-pr 100 --confirm` |

---

## 2. 黄金操作守则 (Golden Principles)

1. **先自省后调用 (Introspect Schema First)**：严禁凭记忆猜测参数名。遇到复杂参数运行 `atlassian-cli schema <command>`（如 `atlassian-cli schema jira comment`）查看严格的 JSON 签名。
2. **写前解析实体 (Resolve Entities Before Writing)**：
   - 查人/@人：`jira user "Name"` ➔ 提取 `[~username]`
   - 查项目合法类型：`jira issue-types --project PROJ`
   - 查合法流转动作：`jira transitions PROJ-123`
   - 查自定义字段字典：`jira fields -q "Sprint"`
   - 查 PR Diff：`bitbucket diff-pr 100 --stat`（先查改动文件清单，再定向查单文件）
3. **写安全防护协议 (Write Safety)**：所有写操作强制传 `--confirm`（未传则 exit 2 拒绝执行）。支持使用 `--dry-run` 预览请求 Payload。

---

## 3. 按需深度手册索引 (On-Demand Deep References)

| 领域范围 | 参考手册相对路径 |
| :--- | :--- |
| JQL 语法、人员提及、附件上传下载、自定义字段、批量/克隆 | `references/jira-commands.md` |
| Confluence 页面获取、HTML 宏保留、原子级查找替换 | `references/confluence-commands.md` |
| Bitbucket PR Diff 预算控制、行级代码评审定位规范 | `references/bitbucket-commands.md` |
| 结构化错误码与退出码自动恢复建议 | `references/error-codes.md` |
| Schema 树自省、幂等性机制与本地审计日志 | `references/advanced.md` |
