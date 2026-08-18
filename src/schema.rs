//! 机器可读的命令树自省 (供 AI Agent 运行时发现命令与参数,离线可用)。
//!
//! 用 clap 的 `CommandFactory` 从 derive 定义反推出完整命令树,
//! 与 CLI 定义零漂移:新增/改名命令自动反映。

use clap::{Arg, Command};
use serde_json::{json, Value};

use crate::error::AppError;

/// 写操作命令表:这些命令需要 --confirm,agent 可通过 schema 的 write:true 预判安全流程。
/// 必须与 module.rs / 三个 api.rs 中实际的 guard 保持一致。
const WRITE_COMMANDS: &[(&str, &str)] = &[
    ("jira", "comment"),
    ("jira", "transition"),
    ("jira", "create"),
    ("jira", "update"),
    ("jira", "assign"),
    ("jira", "worklog-add"),
    ("jira", "worklog-delete"),
    ("jira", "link"),
    ("jira", "attach"),
    ("confluence", "attach"),
    ("confluence", "create"),
    ("confluence", "update"),
    ("bitbucket", "create-pr"),
    ("bitbucket", "comment-pr"),
    ("bitbucket", "approve-pr"),
];

/// 渲染命令树;`path` 为可选过滤路径(如 `["jira", "comment"]`)。
/// 找不到路径返回 `NOT_FOUND`。
pub fn render(cmd: &Command, path: &[String]) -> Result<Value, AppError> {
    let mut current = cmd.clone();
    let mut parent: Option<String> = None;
    for seg in path {
        match current.find_subcommand(seg) {
            Some(sub) => {
                parent = Some(current.get_name().to_string());
                current = sub.clone();
            }
            None => {
                return Err(AppError::not_found(format!("未找到命令: {}", path.join(" "))));
            }
        }
    }
    Ok(render_node(&current, parent.as_deref()))
}

/// 递归渲染单个命令节点;`parent` 为父命令名(用于判定 write 标记)
fn render_node(cmd: &Command, parent: Option<&str>) -> Value {
    let name = cmd.get_name();
    let mut v = json!({
        "name": name,
        "about": cmd.get_about().map(|a| a.to_string()).unwrap_or_default(),
    });

    if parent.map(|p| WRITE_COMMANDS.contains(&(p, name))).unwrap_or(false) {
        v["write"] = json!(true);
    }

    let args: Vec<Value> = cmd
        .get_arguments()
        .filter(|a| {
            let id = a.get_id().as_str();
            id != "help" && id != "version"
        })
        .map(render_arg)
        .collect();
    if !args.is_empty() {
        v["args"] = json!(args);
    }

    let subs: Vec<Value> = cmd.get_subcommands().map(|s| render_node(s, Some(name))).collect();
    if !subs.is_empty() {
        v["subcommands"] = json!(subs);
    }
    v
}

/// 渲染单个参数
fn render_arg(arg: &Arg) -> Value {
    let mut v = json!({
        "name": arg.get_id().to_string(),
        "required": arg.is_required_set(),
        "global": arg.is_global_set(),
    });
    if let Some(long) = arg.get_long() {
        v["long"] = json!(format!("--{}", long));
    }
    if let Some(short) = arg.get_short() {
        v["short"] = json!(format!("-{}", short));
    }
    if let Some(default_val) = arg.get_default_values().first() {
        v["default"] = json!(default_val.to_string_lossy());
    }
    if let Some(help) = arg.get_help() {
        v["help"] = json!(help.to_string());
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Cli;
    use clap::CommandFactory;

    fn full_tree() -> Command {
        Cli::command()
    }

    #[test]
    fn test_render_full_tree() {
        let v = render(&full_tree(), &[]).unwrap();
        assert_eq!(v["name"], "atlassian-cli");
        // 顶层命令存在
        let sub_names: Vec<&str> = v["subcommands"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|s| s["name"].as_str())
            .collect();
        assert!(sub_names.contains(&"jira"));
        assert!(sub_names.contains(&"confluence"));
        assert!(sub_names.contains(&"bitbucket"));
        assert!(sub_names.contains(&"schema"));
    }

    #[test]
    fn test_render_path_filter() {
        let v = render(&full_tree(), &["jira".to_string()]).unwrap();
        assert_eq!(v["name"], "jira");
        assert_eq!(v["about"], "Jira 操作");

        // jira comment 子命令
        let comment = render(&full_tree(), &["jira".to_string(), "comment".to_string()]).unwrap();
        assert_eq!(comment["name"], "comment");
        let arg_names: Vec<&str> = comment["args"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|a| a["name"].as_str())
            .collect();
        assert!(arg_names.contains(&"key"));
        assert!(arg_names.contains(&"body") || arg_names.contains(&"body_pos"));
    }

    #[test]
    fn test_render_arg_details() {
        let jira = render(&full_tree(), &["jira".to_string()]).unwrap();
        let search = jira["subcommands"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["name"] == "search")
            .expect("jira search 存在");
        let limit = search["args"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["name"] == "limit")
            .expect("search --limit 存在");
        assert_eq!(limit["long"], "--limit");
        assert_eq!(limit["default"], "10");
        assert_eq!(limit["required"], false);
    }

    #[test]
    fn test_help_version_filtered() {
        let root = render(&full_tree(), &[]).unwrap();
        let arg_names: Vec<&str> = root["args"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|a| a["name"].as_str())
            .collect();
        assert!(!arg_names.contains(&"help"));
        assert!(!arg_names.contains(&"version"));
        // 全局 flag 保留
        assert!(arg_names.contains(&"insecure"));
        assert!(arg_names.contains(&"dry_run"));
        assert!(arg_names.contains(&"confirm"));
    }

    #[test]
    fn test_render_not_found() {
        let e = render(&full_tree(), &["nonexistent".to_string()]).unwrap_err();
        assert_eq!(e.code, crate::error::ErrorCode::NotFound);
        assert_eq!(e.code.exit_code(), 20);
    }

    #[test]
    fn test_write_marker() {
        // 写操作标 write: true
        let comment = render(&full_tree(), &["jira".to_string(), "comment".to_string()]).unwrap();
        assert_eq!(comment["write"], true);

        let create_pr = render(&full_tree(), &["bitbucket".to_string(), "create-pr".to_string()]).unwrap();
        assert_eq!(create_pr["write"], true);

        let update = render(&full_tree(), &["confluence".to_string(), "update".to_string()]).unwrap();
        assert_eq!(update["write"], true);

        // 读操作无 write 标记
        let get = render(&full_tree(), &["jira".to_string(), "get".to_string()]).unwrap();
        assert!(get.get("write").is_none());

        // 全树中 jira 容器本身无 write
        let jira = render(&full_tree(), &["jira".to_string()]).unwrap();
        assert!(jira.get("write").is_none());
        // 但子树里的 comment 有
        let subs = jira["subcommands"].as_array().unwrap();
        let comment_node = subs.iter().find(|s| s["name"] == "comment").unwrap();
        assert_eq!(comment_node["write"], true);
    }
}
