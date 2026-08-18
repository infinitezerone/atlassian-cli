//! 本地写操作审计日志:记录"谁在何时改了什么"(本机留痕,可追溯 AI 行为)。
//!
//! 文件 `~/.atlassian-cli/audit.jsonl`,与 config.json 同目录。
//! token 永不在其中(PAT 走 HTTP header,不经过 body)。正文摘要截断
//! 200 字符;幂等回放的请求标记 `replayed: true`。
//!
//! 磁盘保护:超过阈值(默认 5MB,`ATLASSIAN_CLI_AUDIT_MAX_BYTES` 可调)
//! 自动滚动到 `audit.1.jsonl`(覆盖旧备份),磁盘占用上限约 2×阈值,
//! 避免像无界日志那样长期运行挤爆磁盘。

use serde_json::{json, Value};

use crate::config;
use crate::error::AppError;

const BODY_PREVIEW_MAX: usize = 200;
const DEFAULT_MAX_BYTES: u64 = 5 * 1024 * 1024;

fn audit_path() -> std::path::PathBuf {
    config::config_dir().join("audit.jsonl")
}

fn max_bytes() -> u64 {
    std::env::var("ATLASSIAN_CLI_AUDIT_MAX_BYTES")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_MAX_BYTES)
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
///
/// 记录同时携带 `hash`(幂等指纹):幂等查询直接读本文件,无需独立的
/// 幂等日志文件——每个写操作只落盘 1 次,减少磁盘 IO 次数与文件数。
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
    rotate_if_needed(log);
    let entry = json!({
        "ts": now_secs(),
        "method": method,
        "path": path,
        "status": status,
        "replayed": replayed,
        "hash": crate::idempotency::fingerprint(method, path, body),
        "body_preview": body_preview(body),
    });
    let mut line = entry.to_string();
    line.push('\n');
    if let Some(parent) = log.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log);
    if let Ok(mut f) = file {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = f.set_permissions(std::fs::Permissions::from_mode(0o600));
        }
        let _ = std::io::Write::write_all(&mut f, line.as_bytes());
    }
}

/// 幂等查询:在主审计日志(+滚动备份)中倒序查找窗口期内相同写请求。
///
/// 追加顺序即时间顺序,倒序遍历遇到第一条超出窗口的记录即可停止
/// (更早的更旧),所以只扫描文件尾部一小段,不会全文件扫描。
pub(crate) fn replay_lookup(
    method: &str,
    path: &str,
    body: Option<&Value>,
    window: u64,
) -> Option<Value> {
    let main = audit_path();
    let backup = main.with_extension("1.jsonl");
    replay_lookup_in(&[&main, &backup], method, path, body, window)
}

fn replay_lookup_in(
    logs: &[&std::path::Path],
    method: &str,
    path: &str,
    body: Option<&Value>,
    window: u64,
) -> Option<Value> {
    let fp = crate::idempotency::fingerprint(method, path, body);
    let now = now_secs();
    for log in logs {
        let Ok(content) = std::fs::read_to_string(log) else { continue };
        for line in content.lines().rev() {
            let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
            let ts = match v["ts"].as_u64() {
                Some(t) => t,
                None => continue,
            };
            if now.saturating_sub(ts) > window {
                break; // 更早的记录更旧,窗口内不会再有
            }
            if v["method"] == method && v["path"] == path && v["hash"] == fp {
                return Some(v);
            }
        }
    }
    None
}

/// 审计日志滚动:当前文件超过阈值时,归档为 `audit.1.jsonl`(覆盖旧备份)。
/// 失败静默(磁盘问题不应影响主流程)。
fn rotate_if_needed(log: &std::path::Path) {
    let Ok(meta) = std::fs::metadata(log) else { return };
    if meta.len() <= max_bytes() {
        return;
    }
    let backup = log.with_extension("1.jsonl");
    let _ = std::fs::remove_file(&backup);
    let _ = std::fs::rename(log, &backup);
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

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
        let _g = ENV_LOCK.lock().unwrap();
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
        let _g = ENV_LOCK.lock().unwrap();
        let log = tmp_log("empty");
        let v = read_in(&log, 10).unwrap();
        assert_eq!(v["count"], 0);
        assert_eq!(v["entries"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_body_preview_truncates() {
        let _g = ENV_LOCK.lock().unwrap();
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
        let _g = ENV_LOCK.lock().unwrap();
        let log = tmp_log("mk").join("sub").join("audit.jsonl");
        append_in(&log, "POST", "/p", "ok", None, false);
        assert!(log.exists());
        let _ = std::fs::remove_dir_all(log.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn test_rotate_on_size_limit() {
        let _g = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("ATLASSIAN_CLI_AUDIT_MAX_BYTES", "50");
        }
        let log = tmp_log("rot");
        append_in(&log, "POST", "/p/1", "ok", None, false);
        append_in(&log, "POST", "/p/2", "ok", None, false);

        let backup = log.with_extension("1.jsonl");
        assert!(backup.exists(), "应已滚动出备份文件");
        let b = std::fs::read_to_string(&backup).unwrap();
        assert!(b.contains("/p/1"), "备份应含第一条记录");
        assert!(!b.contains("/p/2"), "备份不应含第二条记录");
        let m = std::fs::read_to_string(&log).unwrap();
        assert!(m.contains("/p/2"), "主文件应含最新记录");

        // 滚动后主文件是全新文件,只含最新一条
        assert_eq!(m.lines().count(), 1);
        unsafe {
            std::env::remove_var("ATLASSIAN_CLI_AUDIT_MAX_BYTES");
        }
        let _ = std::fs::remove_dir_all(log.parent().unwrap());
    }

    #[test]
    fn test_replay_lookup_matches_window() {
        let _g = ENV_LOCK.lock().unwrap();
        let log = tmp_log("rl");
        let body = json!({ "text": "hello" });
        append_in(&log, "POST", "/rest/api/2/issue/PROJ-1/comment", "ok", Some(&body), false);
        append_in(&log, "DELETE", "/rest/api/2/issue/PROJ-1/worklog/42", "ok", None, false);

        let window = 300u64;
        // 相同请求命中
        assert!(replay_lookup_in(&[&log], "POST", "/rest/api/2/issue/PROJ-1/comment", Some(&body), window).is_some());
        // 不同 path 不命中
        assert!(replay_lookup_in(&[&log], "POST", "/rest/api/2/issue/PROJ-2/comment", Some(&body), window).is_none());
        // 不同 body 不命中
        assert!(replay_lookup_in(&[&log], "POST", "/rest/api/2/issue/PROJ-1/comment", Some(&json!({ "text": "other" })), window).is_none());
        // DELETE 无 body 独立命中
        assert!(replay_lookup_in(&[&log], "DELETE", "/rest/api/2/issue/PROJ-1/worklog/42", None, window).is_some());
        let _ = std::fs::remove_dir_all(log.parent().unwrap());
    }

    #[test]
    fn test_replay_lookup_stops_at_window_boundary() {
        let log = tmp_log("wb");
        let now = now_secs();
        // 手写一条窗口外的记录(在最前面)+ 一条窗口内的
        let stale = json!({
            "ts": now - 1000, "method": "POST", "path": "/old",
            "status": "ok", "replayed": false,
            "hash": crate::idempotency::fingerprint("POST", "/old", Some(&json!({ "a": 1 }))),
            "body_preview": ""
        });
        let fresh = json!({
            "ts": now, "method": "POST", "path": "/new",
            "status": "ok", "replayed": false,
            "hash": crate::idempotency::fingerprint("POST", "/new", Some(&json!({ "b": 2 }))),
            "body_preview": ""
        });
        std::fs::write(&log, format!("{}\n{}\n", stale, fresh)).unwrap();

        // 窗口外的旧记录不应被命中(倒序扫描到窗口边界即停止)
        let window = 300u64;
        assert!(replay_lookup_in(&[&log], "POST", "/old", Some(&json!({ "a": 1 })), window).is_none());
        // 窗口内的新记录正常命中
        assert!(replay_lookup_in(&[&log], "POST", "/new", Some(&json!({ "b": 2 })), window).is_some());
        let _ = std::fs::remove_dir_all(log.parent().unwrap());
    }

    #[test]
    fn test_replay_lookup_reads_backup_after_rotate() {
        let log = tmp_log("bk");
        let body = json!({ "x": 1 });
        // 模拟滚动:主文件归档为备份,新主文件里没有这条记录
        let backup = log.with_extension("1.jsonl");
        append_in(&log, "POST", "/p", "ok", Some(&body), false);
        let _ = std::fs::rename(&log, &backup);

        let window = 300u64;
        // 主文件缺失,但备份里有 → 应命中
        assert!(replay_lookup_in(&[&log, &backup], "POST", "/p", Some(&body), window).is_some());
        // 只给主文件则查不到
        assert!(replay_lookup_in(&[&log], "POST", "/p", Some(&body), window).is_none());
        let _ = std::fs::remove_dir_all(log.parent().unwrap());
    }
}
