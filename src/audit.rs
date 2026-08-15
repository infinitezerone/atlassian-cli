use serde_json::{json, Value};

use crate::config;
use crate::error::AppError;

/// 本地写操作审计日志:记录"谁在何时改了什么"(本机留痕,可追溯 AI 行为)。
///
/// 文件 `~/.atlassian-cli/audit.jsonl`,与 config.json 同目录。
/// token 永不在其中(PAT 走 HTTP header,不经过 body)。正文摘要截断
/// 200 字符;幂等回放的请求标记 `replayed: true`。

const BODY_PREVIEW_MAX: usize = 200;

fn audit_path() -> std::path::PathBuf {
    config::config_dir().join("audit.jsonl")
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn body_preview(body: Option<&Value>) -> String {
    match body {
        None => String::new(),
        Some(b) => {
            let s = b.to_string();
            if s.chars().count() > BODY_PREVIEW_MAX {
                let mut out: String = s.chars().take(BODY_PREVIEW_MAX).collect();
                out.push_str("…(truncated)");
                out
            } else {
                s
            }
        }
    }
}

/// 追加一条审计记录(文件不可写时静默忽略,不影响主流程)。
pub fn append(method: &str, path: &str, status: &str, body: Option<&Value>, replayed: bool) {
    append_in(&audit_path(), method, path, status, body, replayed);
}

fn append_in(
    log: &std::path::Path,
    method: &str,
    path: &str,
    status: &str,
    body: Option<&Value>,
    replayed: bool,
) {
    let entry = json!({
        "ts": now_secs(),
        "method": method,
        "path": path,
        "status": status,
        "replayed": replayed,
        "body_preview": body_preview(body),
    });
    let mut line = entry.to_string();
    line.push('\n');
    if let Some(parent) = log.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
        .and_then(|mut f| std::io::Write::write_all(&mut f, line.as_bytes()));
}

/// 读取最近 N 条审计记录(倒序,最新的在前)。
pub fn read(limit: usize) -> Result<Value, AppError> {
    read_in(&audit_path(), limit)
}

fn read_in(log: &std::path::Path, limit: usize) -> Result<Value, AppError> {
    let content = match std::fs::read_to_string(log) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(AppError::generic(format!("读取审计日志失败: {}", e))),
    };
    let mut entries: Vec<Value> = content
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .collect();
    entries.reverse();
    entries.truncate(limit);
    Ok(json!({
        "status": "ok",
        "count": entries.len(),
        "entries": entries,
        "path": log.display().to_string(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_log(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!(
            "atlassian-cli-audit-test-{}-{}",
            tag,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.join("audit.jsonl")
    }

    #[test]
    fn test_append_and_read_roundtrip() {
        let log = tmp_log("rt");
        append_in(&log, "POST", "/rest/api/2/issue/PROJ-1/comment", "ok", Some(&json!({ "body": "hello" })), false);
        append_in(&log, "DELETE", "/rest/api/2/issue/PROJ-1/worklog/42", "ok", None, false);
        append_in(&log, "POST", "/rest/api/2/issue/PROJ-1/comment", "replayed", Some(&json!({ "body": "hello" })), true);

        let v = read_in(&log, 10).unwrap();
        assert_eq!(v["count"], 3);
        // 倒序:最新的在前
        assert_eq!(v["entries"][0]["status"], "replayed");
        assert_eq!(v["entries"][0]["replayed"], true);
        assert_eq!(v["entries"][2]["method"], "POST");
        assert!(v["entries"][2]["body_preview"].as_str().unwrap().contains("hello"));

        let v2 = read_in(&log, 2).unwrap();
        assert_eq!(v2["count"], 2);
        let _ = std::fs::remove_dir_all(log.parent().unwrap());
    }

    #[test]
    fn test_read_missing_file_returns_empty() {
        let log = tmp_log("empty");
        let v = read_in(&log, 10).unwrap();
        assert_eq!(v["count"], 0);
        assert_eq!(v["entries"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_body_preview_truncates() {
        let long = "x".repeat(500);
        let p = body_preview(Some(&json!({ "text": long })));
        assert!(p.contains("truncated"));
        assert!(p.chars().count() < 300);

        let short = body_preview(Some(&json!({ "text": "hi" })));
        assert!(!short.contains("truncated"));
        assert_eq!(body_preview(None), "");
    }

    #[test]
    fn test_append_missing_dir_creates() {
        let log = tmp_log("mk").join("sub").join("audit.jsonl");
        append_in(&log, "POST", "/p", "ok", None, false);
        assert!(log.exists());
        let _ = std::fs::remove_dir_all(log.parent().unwrap().parent().unwrap());
    }
}
