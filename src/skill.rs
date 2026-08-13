use anyhow::Result;
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;

/// 编译时直接零依赖嵌入 skills/atlassian-cli/SKILL.md
pub const BUILTIN_SKILL: &str = include_str!("../skills/atlassian-cli/SKILL.md");

/// 常见主流 AI Agent 框架与 CLI 的 Skill 部署相对路径清单
const COMMON_SKILL_REL_PATHS: &[&str] = &[
    ".gemini/config/skills/atlassian-cli/SKILL.md", // Google Antigravity / Gemini CLI
    ".claude/skills/atlassian-cli/SKILL.md",        // Claude Code / Anthropic
    ".agents/skills/atlassian-cli/SKILL.md",        // Open-Agents 开放标准
    ".cursor/skills/atlassian-cli/SKILL.md",        // Cursor AI
    ".windsurf/skills/atlassian-cli/SKILL.md",      // Windsurf AI
];

/// 获取所有主流 Agent 的 Skill 绝对目标路径
pub fn get_skill_paths() -> Result<Vec<PathBuf>> {
    let home = dirs_next::home_dir().ok_or_else(|| anyhow::anyhow!("无法获取当前用户 Home 目录"))?;
    Ok(COMMON_SKILL_REL_PATHS.iter().map(|p| home.join(p)).collect())
}

/// 自动将官方 Agent Skill 覆盖同步部署到所有常见的 AI Agent 配置目录
pub fn install_skill() -> Result<Value> {
    let paths = get_skill_paths()?;
    let mut installed_paths: Vec<String> = Vec::new();

    for target_path in &paths {
        if let Some(parent) = target_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if fs::write(target_path, BUILTIN_SKILL).is_ok() {
            let canonical = fs::canonicalize(target_path).unwrap_or(target_path.clone());
            installed_paths.push(canonical.to_string_lossy().to_string());
        }
    }

    Ok(json!({
        "status": "success",
        "action": "install_skill",
        "installed_count": installed_paths.len(),
        "installed_paths": installed_paths,
        "bytes_written_per_file": BUILTIN_SKILL.len(),
        "message": "官方 Agent Skill 已成功同步部署到 Gemini / Antigravity, Claude Code, Open-Agents, Cursor, Windsurf 全量常见 AI Agent 目录！"
    }))
}

/// 检查常见 AI Agent 目录中 Skill 的部署状态
pub fn skill_status() -> Result<Value> {
    let paths = get_skill_paths()?;
    let mut status_list: Vec<Value> = Vec::new();
    let mut installed_count = 0;

    for target_path in &paths {
        let exists = target_path.exists();
        if exists {
            installed_count += 1;
            let canonical = fs::canonicalize(target_path).unwrap_or(target_path.clone());
            let meta = fs::metadata(target_path).ok();
            status_list.push(json!({
                "path": canonical.to_string_lossy(),
                "installed": true,
                "file_size_bytes": meta.map(|m| m.len()).unwrap_or(0),
            }));
        } else {
            status_list.push(json!({
                "path": target_path.to_string_lossy(),
                "installed": false,
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
    }

    #[test]
    fn test_get_skill_paths_count() {
        let paths = get_skill_paths().unwrap();
        assert_eq!(paths.len(), 5);
    }
}
