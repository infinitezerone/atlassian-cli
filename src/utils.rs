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
        let e = ensure_clean_id("key", "A?B").unwrap_err();
        assert_eq!(e.code, crate::error::ErrorCode::ParamInvalid);
    }
}
