use serde_json::{json, Value};

use crate::config;

/// 幂等写操作:窗口期内,相同的写请求 (method + path + body) 只真正执行一次。
///
/// AI 重试/超时重放是常见场景——第一次已成功但结果丢失,重试会重复提交
/// (重复评论/重复工时)。在 HttpClient 的 post/put/delete 出口统一拦截:
/// 命中时返回 `idempotent_replay`(exit 0,视为成功,无副作用)。
///
/// 配置:
/// - `ATLASSIAN_CLI_IDEMPOTENCY_WINDOW` 窗口秒数(默认 300,0 = 关闭)
/// - `ATLASSIAN_CLI_FORCE_WRITE=1` 强制绕过(存量脚本/CI 迁移期用)

const DEFAULT_WINDOW_SECS: u64 = 300;

/// FNV-1a 64 位哈希(零依赖,跨进程稳定,足够去重场景)
fn fnv1a(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 写请求指纹:method + path + 规范化 body(serde_json 的 Map 按 key 排序,
/// 相同 JSON 的 to_string() 稳定,保证跨调用/跨进程一致)。
pub fn fingerprint(method: &str, path: &str, body: Option<&Value>) -> String {
    let mut buf = Vec::with_capacity(method.len() + path.len() + 64);
    buf.extend_from_slice(method.as_bytes());
    buf.push(0);
    buf.extend_from_slice(path.as_bytes());
    buf.push(0);
    if let Some(b) = body {
        buf.extend_from_slice(b.to_string().as_bytes());
    }
    format!("{:016x}", fnv1a(&buf))
}

fn window_secs() -> u64 {
    std::env::var("ATLASSIAN_CLI_IDEMPOTENCY_WINDOW")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_WINDOW_SECS)
}

fn forced() -> bool {
    std::env::var("ATLASSIAN_CLI_FORCE_WRITE")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn log_path() -> std::path::PathBuf {
    config::config_dir().join("idempotency.jsonl")
}

/// 检查窗口期内是否执行过相同写请求;命中返回匹配记录(供 replay_response)。
pub fn check(method: &str, path: &str, body: Option<&Value>) -> Option<Value> {
    check_in(&log_path(), method, path, body)
}

/// 写操作成功后记录指纹(文件不可写时静默忽略,不影响主流程)。
pub fn record(method: &str, path: &str, body: Option<&Value>) {
    record_in(&log_path(), method, path, body);
}

fn check_in(
    log: &std::path::Path,
    method: &str,
    path: &str,
    body: Option<&Value>,
) -> Option<Value> {
    let window = window_secs();
    if window == 0 || forced() {
        return None;
    }
    let fp = fingerprint(method, path, body);
    let content = std::fs::read_to_string(log).ok()?;
    let now = now_secs();
    for line in content.lines().rev() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        if v["method"] == method && v["path"] == path && v["hash"] == fp {
            let ts = v["ts"].as_u64().unwrap_or(0);
            if now.saturating_sub(ts) <= window {
                return Some(v);
            }
        }
    }
    None
}

fn record_in(log: &std::path::Path, method: &str, path: &str, body: Option<&Value>) {
    let entry = json!({
        "ts": now_secs(),
        "method": method,
        "path": path,
        "hash": fingerprint(method, path, body),
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

/// 构造命中响应(HttpClient 直接返回,exit 0,视为成功无副作用)
pub fn replay_response(matched: &Value) -> Value {
    json!({
        "status": "idempotent_replay",
        "action": "skipped",
        "method": matched["method"],
        "path": matched["path"],
        "matched_at": matched["ts"],
        "hint": "窗口期内已执行过相同写操作,已跳过,未重复提交。如需强制重试: ATLASSIAN_CLI_FORCE_WRITE=1"
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn tmp_dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!(
            "atlassian-cli-idem-test-{}-{}",
            tag,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.join("idempotency.jsonl")
    }

    #[test]
    fn test_fingerprint_stable() {
        let body = json!({ "b": 1, "a": 2 });
        let f1 = fingerprint("POST", "/x", Some(&body));
        let f2 = fingerprint("POST", "/x", Some(&body));
        assert_eq!(f1, f2);
        assert_eq!(f1.len(), 16);
        // body 不同则指纹不同
        let f3 = fingerprint("POST", "/x", Some(&json!({ "a": 2, "b": 3 })));
        assert_ne!(f1, f3);
        // 无 body 与有 body 不同
        let f4 = fingerprint("POST", "/x", None);
        assert_ne!(f1, f4);
    }

    #[test]
    fn test_dedupe_roundtrip() {
        let _g = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::remove_var("ATLASSIAN_CLI_FORCE_WRITE");
            std::env::set_var("ATLASSIAN_CLI_IDEMPOTENCY_WINDOW", "300");
        }
        let log = tmp_dir("rt");
        let body = json!({ "text": "hello" });

        assert!(check_in(&log, "POST", "/rest/api/2/issue/PROJ-1/comment", Some(&body)).is_none());
        record_in(&log, "POST", "/rest/api/2/issue/PROJ-1/comment", Some(&body));
        let matched = check_in(&log, "POST", "/rest/api/2/issue/PROJ-1/comment", Some(&body));
        assert!(matched.is_some());
        // 相同 body 不同 path 不命中
        assert!(check_in(&log, "POST", "/rest/api/2/issue/PROJ-2/comment", Some(&body)).is_none());
        // 相同 path 不同 body 不命中
        assert!(check_in(&log, "POST", "/rest/api/2/issue/PROJ-1/comment", Some(&json!({ "text": "other" }))).is_none());
        // DELETE 无 body 独立
        assert!(check_in(&log, "DELETE", "/rest/api/2/issue/PROJ-1/worklog/42", None).is_none());
        record_in(&log, "DELETE", "/rest/api/2/issue/PROJ-1/worklog/42", None);
        assert!(check_in(&log, "DELETE", "/rest/api/2/issue/PROJ-1/worklog/42", None).is_some());
        let _ = std::fs::remove_file(&log);
    }

    #[test]
    fn test_window_zero_disables() {
        let _g = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::remove_var("ATLASSIAN_CLI_FORCE_WRITE");
            std::env::set_var("ATLASSIAN_CLI_IDEMPOTENCY_WINDOW", "0");
        }
        let log = tmp_dir("w0");
        let body = json!({ "x": 1 });
        record_in(&log, "POST", "/p", Some(&body));
        assert!(check_in(&log, "POST", "/p", Some(&body)).is_none()); // 窗口为 0 视为关闭
        let _ = std::fs::remove_file(&log);
    }

    #[test]
    fn test_force_write_bypasses() {
        let _g = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("ATLASSIAN_CLI_FORCE_WRITE", "1");
            std::env::set_var("ATLASSIAN_CLI_IDEMPOTENCY_WINDOW", "300");
        }
        let log = tmp_dir("fw");
        let body = json!({ "x": 1 });
        record_in(&log, "POST", "/p", Some(&body));
        assert!(check_in(&log, "POST", "/p", Some(&body)).is_none()); // 强制绕过
        let _ = std::fs::remove_file(&log);
    }

    #[test]
    fn test_replay_response_structure() {
        let matched = json!({ "ts": 100, "method": "POST", "path": "/x" });
        let v = replay_response(&matched);
        assert_eq!(v["status"], "idempotent_replay");
        assert_eq!(v["action"], "skipped");
        assert_eq!(v["path"], "/x");
        assert!(v["hint"].as_str().unwrap().contains("FORCE_WRITE"));
    }
}
