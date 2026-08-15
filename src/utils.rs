use crate::error::AppError;

/// 拒绝含控制字符 / 查询参数残留的资源 ID(防 Agent 幻觉输入直接注入 URL)。
/// 调用时机:在 parse_* 之后、发起 HTTP 之前。
pub fn ensure_clean_id(kind: &str, id: &str) -> Result<(), AppError> {
    let has_ctrl = id.chars().any(|c| c.is_control());
    let has_query = id.contains('?') || id.contains('#');
    if has_ctrl || has_query {
        return Err(AppError::param_invalid(format!("{} 含非法字符: '{}'", kind, id)));
    }
    Ok(())
}

/// 本地 JQL 语法级校验(非空 + 括号配对 + 引号配对)。
///
/// 不做完整文法解析(那需要完整 JQL 文法),只拦截 AI 拼查询时最常见的
/// 低级错误:空查询、括号不配对、字符串未闭合。校验失败返回
/// `PARAM_INVALID` 并给出可执行的修正建议。
pub fn validate_jql(jql: &str) -> Result<(), AppError> {
    let j = jql.trim();
    if j.is_empty() {
        return Err(AppError::param_invalid("JQL 为空")
            .with_suggestion("例如: atlassian-cli jira search \"project = PROJSA AND status != Closed\""));
    }

    let mut depth = 0i32;
    for (i, c) in j.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth < 0 {
                    return Err(AppError::param_invalid(format!("JQL 括号不匹配: 第 {} 字符处多了一个 ')'", i + 1))
                        .with_suggestion("检查 AND/OR 分组的括号是否配对,例如: (assignee = currentUser()) AND status != Closed"));
                }
            }
            _ => {}
        }
    }
    if depth > 0 {
        return Err(AppError::param_invalid(format!("JQL 括号不匹配: 缺少 {} 个 ')'", depth))
            .with_suggestion("补全括号后重试;可用 atlassian-cli jira suggest-fields 查询可用字段"));
    }

    let mut in_str: Option<char> = None;
    let mut it = j.char_indices().peekable();
    while let Some((_, c)) = it.next() {
        if let Some(q) = in_str {
            if c == '\\' {
                let _ = it.next();
                continue;
            }
            if c == q {
                in_str = None;
            }
        } else if c == '\'' || c == '"' {
            in_str = Some(c);
        }
    }
    if let Some(q) = in_str {
        return Err(AppError::param_invalid(format!("JQL 字符串未闭合: 缺少匹配的 '{}'", q))
            .with_suggestion("字符串值请用引号包住,例如: summary ~ \"登录超时\""));
    }
    Ok(())
}

/// 校验 Jira 工时格式 (如 "2h 30m" / "1d" / "45m" / "3w")。
///
/// Jira 合法单位: w(周) d(天) h(小时) m(分钟), 可组合且单位不重复。
/// 返回 `PARAM_INVALID` 并提示正确格式。
pub fn validate_time_spent(input: &str) -> Result<(), AppError> {
    let s = input.trim();
    if s.is_empty() {
        return Err(AppError::param_invalid("工时不能为空")
            .with_suggestion("例如: atlassian-cli jira worklog-add PROJSA-123 \"2h 30m\" --confirm"));
    }
    let mut seen = std::collections::HashSet::new();
    let mut rest = s;
    let mut matched_any = false;
    while !rest.is_empty() {
        let num_end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
        if num_end == 0 {
            return Err(AppError::param_invalid(format!("工时格式非法: '{}'", input))
                .with_suggestion("正确格式示例: \"45m\" / \"2h\" / \"2h 30m\" / \"1d\" / \"3w\" (单位 w/d/h/m 组合, 每单位只能用一次)"));
        }
        let _num = &rest[..num_end];
        let unit_char = rest[num_end..].chars().next();
        let (unit, consumed) = match unit_char {
            Some('w') => ("w", 1),
            Some('d') => ("d", 1),
            Some('h') => ("h", 1),
            Some('m') => ("m", 1),
            _ => {
                return Err(AppError::param_invalid(format!("工时单位非法: '{}'", input))
                    .with_suggestion("合法单位: w (周) / d (天) / h (小时) / m (分钟)"));
            }
        };
        if !seen.insert(unit) {
            return Err(AppError::param_invalid(format!("工时单位重复: '{}'", input))
                .with_suggestion("同一单位只能用一次,例如 \"2h 30m\" 而不是 \"1h 2h\""));
        }
        matched_any = true;
        rest = &rest[num_end + consumed..];
        rest = rest.trim_start();
    }
    if !matched_any {
        return Err(AppError::param_invalid(format!("工时格式非法: '{}'", input))
            .with_suggestion("正确格式示例: \"45m\" / \"2h\" / \"2h 30m\" / \"1d\" / \"3w\""));
    }
    Ok(())
}

/// 校验 worklog 开始时间格式: "YYYY-MM-DD" 或 "YYYY-MM-DDTHH:MM:SS"
pub fn validate_started(input: &str) -> Result<(), AppError> {
    let s = input.trim();
    // 只允许 ASCII 数字 / 连字符 / T / 冒号,避免后续字节切片遇到多字节字符
    if s.chars().any(|c| !(c.is_ascii_digit() || c == '-' || c == 'T' || c == ':')) {
        return Err(AppError::param_invalid(format!("开始时间格式非法: '{}'", input))
            .with_suggestion("支持格式: \"2026-08-15\" 或 \"2026-08-15T09:30:00\""));
    }
    let date_part = s.split('T').next().unwrap_or(s);
    if date_part.len() != 10 || &date_part[4..5] != "-" || &date_part[7..8] != "-" {
        return Err(AppError::param_invalid(format!("开始时间格式非法: '{}'", input))
            .with_suggestion("支持格式: \"2026-08-15\" 或 \"2026-08-15T09:30:00\""));
    }
    let y = &date_part[0..4];
    let m = &date_part[5..7];
    let d = &date_part[8..10];
    if !y.chars().all(|c| c.is_ascii_digit())
        || !m.chars().all(|c| c.is_ascii_digit())
        || !d.chars().all(|c| c.is_ascii_digit())
    {
        return Err(AppError::param_invalid(format!("开始时间格式非法: '{}'", input))
            .with_suggestion("支持格式: \"2026-08-15\" 或 \"2026-08-15T09:30:00\""));
    }
    // 数值范围检查(拦 AI 常见的 13 月 / 32 日)
    let m_num: u32 = m.parse().unwrap_or(0);
    let d_num: u32 = d.parse().unwrap_or(0);
    if !(1..=12).contains(&m_num) || !(1..=31).contains(&d_num) {
        return Err(AppError::param_invalid(format!("开始时间数值非法: '{}-{}-{}'", y, m, d))
            .with_suggestion("月份 1-12,日期 1-31,例如: \"2026-08-15\""));
    }
    if let Some(time_part) = s.split('T').nth(1) {
        let len_ok = time_part.len() == 8 || time_part.len() == 5;
        let colon_ok = time_part.len() == 8
            && (&time_part[2..3] == ":" && &time_part[5..6] == ":");
        let short_ok = time_part.len() == 5 && &time_part[2..3] == ":";
        if !len_ok || !(colon_ok || short_ok) {
            return Err(AppError::param_invalid(format!("开始时间格式非法: '{}'", input))
                .with_suggestion("支持格式: \"2026-08-15\" 或 \"2026-08-15T09:30:00\""));
        }
    }
    Ok(())
}

/// 校验评论文本中的 Jira 提及语法 `[~username]`。
///
/// 只拦截显式 `[~...]` 结构的格式错误(空提及 / 内含空格 / 未闭合),
/// 裸 `@` 文本不拦截(在 Jira 中只是纯文本,不会渲染成提及)。
/// 校验失败时建议先用 `atlassian-cli jira user` 查询真实 mention_syntax。
pub fn validate_mentions(text: &str) -> Result<(), AppError> {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'[' && bytes[i + 1] == b'~' {
            let close = text[i..].find(']');
            match close {
                None => {
                    return Err(AppError::param_invalid("评论文本中的提及未闭合: 缺少 ']'")
                        .with_suggestion("正确语法: [~username];先用 atlassian-cli jira user \"名字\" 查询真实 mention_syntax"));
                }
                Some(rel) => {
                    let inner = &text[i + 2..i + rel];
                    if inner.trim().is_empty() {
                        return Err(AppError::param_invalid("评论文本中的提及为空: [~]")
                            .with_suggestion("正确语法: [~username];先用 atlassian-cli jira user \"名字\" 查询真实 mention_syntax"));
                    }
                    if inner.chars().any(|c| c.is_whitespace()) {
                        return Err(AppError::param_invalid(format!("评论文本中的提及含空格: '[~{}]'", inner))
                            .with_suggestion("Jira 用户名不含空格;先用 atlassian-cli jira user \"名字\" 查询真实 mention_syntax"));
                    }
                    i += rel + 1;
                    continue;
                }
            }
        }
        i += 1;
    }
    Ok(())
}

/// 从 Jira Issue Key 或完整网页 URL 中提取 Issue Key
///
/// 支持格式:
/// - `PROJ-1234` → `PROJ-1234`
/// - `https://jira.example.com/jira/browse/PROJ-1234` → `PROJ-1234`
pub fn parse_jira_key(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        if let Some(idx) = trimmed.find("/browse/") {
            let part = &trimmed[idx + "/browse/".len()..];
            let clean = part.split(&['/', '?', '#'][..]).next().unwrap_or(part);
            return clean.to_string();
        }
    }
    trimmed.to_string()
}

/// 从 Confluence Page ID 或完整网页 URL 中提取 Page ID
///
/// 支持格式:
/// - `123456` → `123456`
/// - `https://confluence.example.com/confluence/pages/viewpage.action?pageId=123456` → `123456`
/// - `https://confluence.example.com/confluence/display/SPACE/Title` → 从 `spaceKey+title` 中提取
pub fn parse_confluence_id(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        // 优先从 pageId= 查询参数提取
        if let Some(idx) = trimmed.find("pageId=") {
            let part = &trimmed[idx + "pageId=".len()..];
            let id = part.split(&['&', '#'][..]).next().unwrap_or(part);
            if !id.is_empty() {
                return id.to_string();
            }
        }
        // 其次从路径 /pages/{id}/ 提取
        if let Some(idx) = trimmed.find("/pages/") {
            let part = &trimmed[idx + "/pages/".len()..];
            let id = part.split(&['/', '?', '#'][..]).next().unwrap_or(part);
            if !id.is_empty() {
                return id.to_string();
            }
        }
    }
    trimmed.to_string()
}

/// 从 Bitbucket PR 的 ID/URL 解析出 (project, repo, pr_id)
///
/// 支持格式:
/// - `(--project PROJ, --repo my-repo, 2420)` → `("PROJ", "my-repo", "2420")`
/// - 完整网页 URL → 自动解析
pub fn parse_bitbucket_pr(
    id_or_url: &str,
    project: Option<&str>,
    repo: Option<&str>,
) -> Result<(String, String, String), AppError> {
    let trimmed = id_or_url.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        let proj_idx = trimmed
            .find("/projects/")
            .ok_or_else(|| AppError::param_invalid("URL 中未找到 /projects/ 路径"))?;
        let repo_idx = trimmed
            .find("/repos/")
            .ok_or_else(|| AppError::param_invalid("URL 中未找到 /repos/ 路径"))?;
        let pr_idx = trimmed
            .find("/pull-requests/")
            .ok_or_else(|| AppError::param_invalid("URL 中未找到 /pull-requests/ 路径"))?;

        let p = &trimmed[proj_idx + "/projects/".len()..repo_idx];
        let r = &trimmed[repo_idx + "/repos/".len()..pr_idx];
        let pr_part = &trimmed[pr_idx + "/pull-requests/".len()..];
        let id = pr_part.split(&['/', '?', '#'][..]).next().unwrap_or(pr_part);

        if p.is_empty() || r.is_empty() || id.is_empty() {
            return Err(AppError::param_invalid("无法从 URL 中解析出有效的 Project, Repo 或 PR ID"));
        }
        return Ok((p.to_string(), r.to_string(), id.to_string()));
    }

    let p = project.unwrap_or("").trim();
    let r = repo.unwrap_or("").trim();
    if p.is_empty() || r.is_empty() {
        return Err(AppError::param_invalid("未传入完整 PR 网页 URL 时，必须提供 --project 和 --repo 参数"));
    }
    Ok((p.to_string(), r.to_string(), trimmed.to_string()))
}

/// 从 Bitbucket 仓库 URL 或 (project, repo) 参数提取 (project, repo)
pub fn parse_bitbucket_repo(
    repo_url: Option<&str>,
    project: Option<&str>,
    repo: Option<&str>,
) -> Result<(String, String), AppError> {
    if let Some(input) = repo_url {
        let trimmed = input.trim();
        if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            let proj_idx = trimmed
                .find("/projects/")
                .ok_or_else(|| AppError::param_invalid("URL 中未找到 /projects/ 路径"))?;
            let repo_idx = trimmed
                .find("/repos/")
                .ok_or_else(|| AppError::param_invalid("URL 中未找到 /repos/ 路径"))?;

            let p = &trimmed[proj_idx + "/projects/".len()..repo_idx];
            let after_repo = &trimmed[repo_idx + "/repos/".len()..];
            let r = after_repo.split(&['/', '?', '#'][..]).next().unwrap_or(after_repo);

            if !p.is_empty() && !r.is_empty() {
                return Ok((p.to_string(), r.to_string()));
            }
        }
    }

    let p = project.unwrap_or("").trim();
    let r = repo.unwrap_or("").trim();
    if p.is_empty() || r.is_empty() {
        return Err(AppError::param_invalid("必须提供 --project 和 --repo 参数（或传入 --url 完整仓库/PR 网页链接）"));
    }
    Ok((p.to_string(), r.to_string()))
}

/// 从传入的用户名字符串中智能剥离 [~username] / @{username} / @username 等装饰标记
///
/// 支持格式:
/// - `john.doe` → `john.doe`
/// - `[~john.doe]` → `john.doe`
/// - `@{john.doe}` → `john.doe`
/// - `@john.doe` → `john.doe`
pub fn parse_username(input: &str) -> String {
    let mut trimmed = input.trim();
    if trimmed.starts_with("[~") && trimmed.ends_with(']') {
        trimmed = &trimmed[2..trimmed.len() - 1];
    } else if trimmed.starts_with("@{") && trimmed.ends_with('}') {
        trimmed = &trimmed[2..trimmed.len() - 1];
    } else if trimmed.starts_with('@') {
        trimmed = &trimmed[1..];
    }
    trimmed.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_username() {
        assert_eq!(parse_username("john.doe"), "john.doe");
        assert_eq!(parse_username("[~john.doe]"), "john.doe");
        assert_eq!(parse_username("@{john.doe}"), "john.doe");
        assert_eq!(parse_username("@john.doe"), "john.doe");
        assert_eq!(parse_username("  [~john.doe]  "), "john.doe");
    }

    #[test]
    fn test_ensure_clean_id() {
        assert!(ensure_clean_id("key", "PROJ-123").is_ok());
        assert!(ensure_clean_id("key", "123456").is_ok());
        assert!(ensure_clean_id("key", "PROJ-123?x=1").is_err());
        assert!(ensure_clean_id("key", "PROJ-123#frag").is_err());
        assert!(ensure_clean_id("key", "PROJ\u{0000}").is_err());
    }

    #[test]
    fn test_validate_jql() {
        assert!(validate_jql("assignee = currentUser() AND status != Closed").is_ok());
        assert!(validate_jql("project = PROJSA ORDER BY created DESC").is_ok());
        assert!(validate_jql("summary ~ \"登录超时\"").is_ok());
        assert!(validate_jql("").is_err());
        assert!(validate_jql("   ").is_err());
        assert!(validate_jql("(a = b").is_err());       // 缺右括号
        assert!(validate_jql("a = b)").is_err());       // 多右括号
        assert!(validate_jql("summary ~ \"abc").is_err()); // 引号未闭合
        assert!(validate_jql("summary ~ 'abc").is_err());
        assert!(validate_jql("text ~ \"it's ok\"").is_ok()); // 转义内引号
    }

    #[test]
    fn test_validate_time_spent() {
        assert!(validate_time_spent("45m").is_ok());
        assert!(validate_time_spent("2h").is_ok());
        assert!(validate_time_spent("2h 30m").is_ok());
        assert!(validate_time_spent("1d").is_ok());
        assert!(validate_time_spent("3w").is_ok());
        assert!(validate_time_spent("1w 2d 3h 4m").is_ok());
        assert!(validate_time_spent("").is_err());
        assert!(validate_time_spent("abc").is_err());
        assert!(validate_time_spent("2x").is_err());       // 非法单位
        assert!(validate_time_spent("1h 2h").is_err());    // 单位重复
        assert!(validate_time_spent("2h30").is_err());     // 缺单位
    }

    #[test]
    fn test_validate_started() {
        assert!(validate_started("2026-08-15").is_ok());
        assert!(validate_started("2026-08-15T09:30:00").is_ok());
        assert!(validate_started("2026-08-15T09:30").is_ok());
        assert!(validate_started("2026/08/15").is_err());
        assert!(validate_started("2026-13-45").is_err()); // 只是格式层,不做日历校验
        assert!(validate_started("15-08-2026").is_err());
        assert!(validate_started("2026-08-15T09:30:xx").is_err());
    }

    #[test]
    fn test_validate_mentions() {
        assert!(validate_mentions("普通评论,无提及").is_ok());
        assert!(validate_mentions("联系 [~john.doe] 处理").is_ok());
        assert!(validate_mentions("邮箱 a@b.com 不受影响").is_ok());
        assert!(validate_mentions("[~john doe] 有空格").is_err());
        assert!(validate_mentions("[~] 空提及").is_err());
        assert!(validate_mentions("[~john.doe 未闭合").is_err());
    }
}
