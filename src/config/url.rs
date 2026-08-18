/// 智能 URL 归一化: 自动去除前后空格、补全 https:// 前缀、删除末尾斜杠、智能裁剪网页路径 (如 /browse/..., /pages/..., /projects/...)
pub fn normalize_module_url(input: &str, module: &str) -> String {
    let mut trimmed = input.trim().to_string();
    if trimmed.is_empty() {
        return String::new();
    }
    if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
        trimmed = format!("https://{}", trimmed);
    }
    if let Some(pos) = trimmed.find('?') {
        trimmed.truncate(pos);
    }
    if let Some(pos) = trimmed.find('#') {
        trimmed.truncate(pos);
    }
    while trimmed.ends_with('/') {
        trimmed.pop();
    }

    let markers: &[&str] = match module.to_lowercase().as_str() {
        "jira" => &["/browse/", "/secure/", "/projects/", "/issues/", "/rest/api/"],
        "confluence" => &["/pages/", "/display/", "/spaces/", "/rest/api/"],
        "bitbucket" => &["/projects/", "/users/", "/scm/", "/plugins/servlet/", "/rest/api/"],
        _ => &["/browse/", "/pages/", "/display/", "/projects/"],
    };

    for marker in markers {
        if let Some(pos) = trimmed.find(marker) {
            trimmed.truncate(pos);
            break;
        }
    }

    while trimmed.ends_with('/') {
        trimmed.pop();
    }
    trimmed
}

#[allow(dead_code)]
pub fn normalize_url(input: &str) -> String {
    normalize_module_url(input, "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_url() {
        assert_eq!(normalize_url("jira.example.com"), "https://jira.example.com");
        assert_eq!(normalize_url("  jira.example.com/  "), "https://jira.example.com");
        assert_eq!(normalize_url("http://jira.example.com/jira/"), "http://jira.example.com/jira");
        assert_eq!(normalize_url("https://gitpub.company.com"), "https://gitpub.company.com");
    }

    #[test]
    fn test_normalize_module_url_smart_strip() {
        // Jira 单子 URL
        assert_eq!(
            normalize_module_url("https://jira.company.com/browse/PROJ-123", "jira"),
            "https://jira.company.com"
        );
        assert_eq!(
            normalize_module_url("https://company.com/jira/browse/PROJ-123?filter=1", "jira"),
            "https://company.com/jira"
        );
        assert_eq!(
            normalize_module_url("https://jira.company.com/secure/Dashboard.jspa", "jira"),
            "https://jira.company.com"
        );

        // Confluence 页面 URL
        assert_eq!(
            normalize_module_url("https://confluence.company.com/pages/viewpage.action?pageId=123", "confluence"),
            "https://confluence.company.com"
        );
        assert_eq!(
            normalize_module_url("https://company.com/confluence/display/SPACE/Title", "confluence"),
            "https://company.com/confluence"
        );

        // Bitbucket PR 与仓库 URL
        assert_eq!(
            normalize_module_url("https://bitbucket.company.com/projects/PROJ/repos/repo/pull-requests/1", "bitbucket"),
            "https://bitbucket.company.com"
        );
        assert_eq!(
            normalize_module_url("https://company.com/bitbucket/projects/PROJ/repos/repo", "bitbucket"),
            "https://company.com/bitbucket"
        );
    }
}
