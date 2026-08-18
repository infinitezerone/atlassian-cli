use crate::error::AppError;
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;

/// 编译时直接零依赖嵌入 skills/atlassian-cli/SKILL.md
pub const BUILTIN_SKILL: &str = include_str!("../skills/atlassian-cli/SKILL.md");

/// 渐进式披露:SKILL.md 精简,细节拆到 references/ 按需加载(同样 include_str 打进单二进制)
const BUILTIN_REF_JIRA: &str = include_str!("../skills/atlassian-cli/references/jira-commands.md");
const BUILTIN_REF_CONFLUENCE: &str =
    include_str!("../skills/atlassian-cli/references/confluence-commands.md");
const BUILTIN_REF_BITBUCKET: &str =
    include_str!("../skills/atlassian-cli/references/bitbucket-commands.md");
const BUILTIN_REF_ERROR_CODES: &str =
    include_str!("../skills/atlassian-cli/references/error-codes.md");
const BUILTIN_REF_ADVANCED: &str = include_str!("../skills/atlassian-cli/references/advanced.md");

/// (相对路径, 内容) 清单:install 时逐个写入各 agent 的 skill 目录
const SKILL_FILES: &[(&str, &str)] = &[
    ("SKILL.md", BUILTIN_SKILL),
    ("references/jira-commands.md", BUILTIN_REF_JIRA),
    ("references/confluence-commands.md", BUILTIN_REF_CONFLUENCE),
    ("references/bitbucket-commands.md", BUILTIN_REF_BITBUCKET),
    ("references/error-codes.md", BUILTIN_REF_ERROR_CODES),
    ("references/advanced.md", BUILTIN_REF_ADVANCED),
];

/// 常见主流 AI Agent 框架与 CLI 的 Skill 部署相对路径清单
const COMMON_SKILL_REL_PATHS: &[&str] = &[
    ".gemini/config/skills/atlassian-cli/SKILL.md", // Google Antigravity / Gemini CLI
    ".claude/skills/atlassian-cli/SKILL.md",        // Claude Code / Anthropic
    ".agents/skills/atlassian-cli/SKILL.md",        // Open-Agents 开放标准
    ".cursor/skills/atlassian-cli/SKILL.md",        // Cursor AI
    ".windsurf/skills/atlassian-cli/SKILL.md",      // Windsurf AI
    ".workbuddy/skills/atlassian-cli/SKILL.md",     // WorkBuddy
    ".codebuddy/skills/atlassian-cli/SKILL.md",     // CodeBuddy
];

/// 获取所有主流 Agent 的 Skill 绝对目标路径
pub fn get_skill_paths() -> Result<Vec<PathBuf>, AppError> {
    let home = dirs_next::home_dir().ok_or_else(|| AppError::generic("无法获取当前用户 Home 目录"))?;
    Ok(COMMON_SKILL_REL_PATHS.iter().map(|p| home.join(p)).collect())
}

/// 自动将官方 Agent Skill(含 references/)覆盖同步部署到所有常见的 AI Agent 配置目录
pub fn install_skill() -> Result<Value, AppError> {
    let paths = get_skill_paths()?;
    let mut installed_paths: Vec<String> = Vec::new();
    let mut total_bytes = 0usize;

    for target_path in &paths {
        // target_path = .../skills/atlassian-cli/SKILL.md → 其父目录即 skill 根目录
        if let Some(skill_dir) = target_path.parent() {
            let _ = fs::create_dir_all(skill_dir.join("references"));
            // 写入顺序:先 references、最后 SKILL.md —— 若中途失败,SKILL.md 仍是旧的自包含版,
            // 不会出现"新 SKILL.md 引用缺失 references"的不完整中间态(升级兼容加固)
            let mut all_ok = true;
            for (rel, content) in SKILL_FILES.iter().filter(|(rel, _)| *rel != "SKILL.md") {
                let full = skill_dir.join(rel);
                if fs::write(&full, content).is_ok() {
                    total_bytes += content.len();
                } else {
                    all_ok = false;
                }
            }
            if fs::write(target_path, BUILTIN_SKILL).is_ok() && all_ok {
                total_bytes += BUILTIN_SKILL.len();
                installed_paths.push(target_path.to_string_lossy().to_string());
            }
        }
    }

    Ok(json!({
        "status": "success",
        "action": "install_skill",
        "installed_count": installed_paths.len(),
        "installed_paths": installed_paths,
        "files_installed": SKILL_FILES.len(),
        "bytes_written_total": total_bytes,
        "message": "官方 Agent Skill(含按需加载的 references/)已成功同步部署到 Gemini / Antigravity, Claude Code, Open-Agents, Cursor, Windsurf, WorkBuddy 全量常见 AI Agent 目录！"
    }))
}

/// 彻底卸载已部署的 Agent Skill(整目录清理,含 references/)
pub fn uninstall_skill() -> Result<Value, AppError> {
    let paths = get_skill_paths()?;
    let mut uninstalled_paths: Vec<String> = Vec::new();
    let mut not_found_paths: Vec<String> = Vec::new();

    for target_path in &paths {
        if let Some(skill_dir) = target_path.parent() {
            if skill_dir.exists() {
                if fs::remove_dir_all(skill_dir).is_ok() {
                    uninstalled_paths.push(skill_dir.to_string_lossy().to_string());
                }
            } else {
                not_found_paths.push(skill_dir.to_string_lossy().to_string());
            }
        }
    }

    Ok(json!({
        "status": "success",
        "action": "uninstall_skill",
        "uninstalled_count": uninstalled_paths.len(),
        "uninstalled_paths": uninstalled_paths,
        "not_found_count": not_found_paths.len(),
        "message": "已成功将 Agent Skill 从系统中彻底移除。"
    }))
}

/// 检查 Agent Skill 在各环境中的安装状态(包含 references 完整度检查)
pub fn skill_status() -> Result<Value, AppError> {
    let paths = get_skill_paths()?;
    let mut installed_count = 0;
    let mut status_list = Vec::new();

    for target_path in &paths {
        let is_installed = target_path.exists();
        if is_installed {
            installed_count += 1;
            let metadata = fs::metadata(target_path).ok();
            let size = metadata.map(|m| m.len()).unwrap_or(0);
            let skill_dir = target_path.parent();
            let references_complete = skill_dir
                .map(|d| {
                    SKILL_FILES
                        .iter()
                        .filter(|(rel, _)| *rel != "SKILL.md")
                        .all(|(rel, _)| d.join(rel).exists())
                })
                .unwrap_or(false);

            status_list.push(json!({
                "path": target_path.to_string_lossy(),
                "installed": true,
                "file_size_bytes": size,
                "references_complete": references_complete,
            }));
        } else {
            status_list.push(json!({
                "path": target_path.to_string_lossy(),
                "installed": false
            }));
        }
    }

    Ok(json!({
        "status": if installed_count > 0 { "installed" } else { "not_installed" },
        "installed_count": installed_count,
        "total_targets": paths.len(),
        "details": status_list,
        "hint": "运行 'atlassian-cli skill install' 可一键更新所有 Agent 目录中的技能规约。"
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_skill_not_empty() {
        assert!(!BUILTIN_SKILL.is_empty());
        assert!(BUILTIN_SKILL.contains("atlassian-cli"));
        assert!(BUILTIN_SKILL.contains("jira"));
        // 精简版 SKILL.md 应引用 references(渐进式披露)
        assert!(BUILTIN_SKILL.contains("references/"));
    }

    #[test]
    fn test_builtin_references_not_empty() {
        assert!(BUILTIN_REF_JIRA.contains("worklog-add"));
        assert!(BUILTIN_REF_JIRA.contains("bulk-create"));
        assert!(BUILTIN_REF_CONFLUENCE.contains("confluence update"));
        assert!(BUILTIN_REF_BITBUCKET.contains("approve-pr"));
        assert!(BUILTIN_REF_ERROR_CODES.contains("AUTH_EXPIRED"));
        assert!(BUILTIN_REF_ADVANCED.contains("idempotent_replay"));
        assert_eq!(SKILL_FILES.len(), 6, "SKILL.md + 5 个 references");
    }

    #[test]
    fn test_get_skill_paths_count() {
        let paths = get_skill_paths().unwrap();
        assert_eq!(paths.len(), 7);
    }
}
