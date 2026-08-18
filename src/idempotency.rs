use serde_json::{json, Value};

/// 幂等写操作:窗口期内,相同的写请求 (method + path + body) 只真正执行一次。
///
/// AI 重试/超时重放是常见场景——第一次已成功但结果丢失,重试会重复提交
/// (重复评论/重复工时)。在 HttpClient 的 post/put/delete 出口统一拦截:
/// 命中时返回 `idempotent_replay`(exit 0,视为成功,无副作用)。
///
/// 记录复用审计日志(每条审计记录携带 hash 指纹,见 audit.rs),**不写
/// 独立的幂等文件**——每个写操作只有一次磁盘追加,减少 IO 次数。
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

/// 检查窗口期内是否执行过相同写请求(查询审计日志,不产生独立落盘);
/// 命中返回匹配记录(供 replay_response)。
pub fn check(method: &str, path: &str, body: Option<&Value>) -> Option<Value> {
    let window = window_secs();
    if window == 0 || forced() {
        return None;
    }
    crate::audit::replay_lookup(method, path, body, window)
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
    fn test_check_disabled_when_window_zero() {
        let _g = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::remove_var("ATLASSIAN_CLI_FORCE_WRITE");
            std::env::set_var("ATLASSIAN_CLI_IDEMPOTENCY_WINDOW", "0");
        }
        let body = json!({ "x": 1 });
        assert!(check("POST", "/p", Some(&body)).is_none()); // 窗口 0 = 关闭,不查询
        unsafe {
            std::env::remove_var("ATLASSIAN_CLI_IDEMPOTENCY_WINDOW");
        }
    }

    #[test]
    fn test_check_forced_bypasses() {
        let _g = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("ATLASSIAN_CLI_FORCE_WRITE", "1");
            std::env::set_var("ATLASSIAN_CLI_IDEMPOTENCY_WINDOW", "300");
        }
        let body = json!({ "x": 1 });
        assert!(check("POST", "/p", Some(&body)).is_none()); // 强制绕过,不查询
        unsafe {
            std::env::remove_var("ATLASSIAN_CLI_FORCE_WRITE");
            std::env::remove_var("ATLASSIAN_CLI_IDEMPOTENCY_WINDOW");
        }
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
