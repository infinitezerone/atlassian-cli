//! 结构化错误:机器可读的错误码 + 细粒度退出码 + Agent 可执行的建议。
//!
//! 策略:公共边界(http、模块 trait、config、main 收口)统一返回 `Result<_, AppError>`;
//! 内部实现仍可自由使用 `anyhow` 做 context,通过 `From<anyhow::Error>` 兜底映射。

use std::fmt;

/// 结构化错误码
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    /// HTTP 401: PAT 无效/过期
    AuthExpired,
    /// HTTP 403: 权限拒绝
    PermissionDenied,
    /// HTTP 404 / 业务资源未找到
    NotFound,
    /// 本地参数/校验错误(含缺失 --confirm)
    ParamInvalid,
    /// URL/Token 未配置或配置不可读
    ConfigMissing,
    /// 其他 HTTP/网络/解析错误
    HttpError,
    /// 兜底
    Generic,
}

impl ErrorCode {
    /// 机器可读的 code 字符串
    pub fn as_str(&self) -> &'static str {
        match self {
            ErrorCode::AuthExpired => "AUTH_EXPIRED",
            ErrorCode::PermissionDenied => "PERMISSION_DENIED",
            ErrorCode::NotFound => "NOT_FOUND",
            ErrorCode::ParamInvalid => "PARAM_INVALID",
            ErrorCode::ConfigMissing => "CONFIG_MISSING",
            ErrorCode::HttpError => "HTTP_ERROR",
            ErrorCode::Generic => "UNKNOWN_ERROR",
        }
    }

    /// 进程退出码:0 成功、2 参数、3 配置缺失、10 认证、11 权限、20 未找到、1 兜底
    pub fn exit_code(&self) -> i32 {
        match self {
            ErrorCode::AuthExpired => 10,
            ErrorCode::PermissionDenied => 11,
            ErrorCode::NotFound => 20,
            ErrorCode::ParamInvalid => 2,
            ErrorCode::ConfigMissing => 3,
            ErrorCode::HttpError => 1,
            ErrorCode::Generic => 1,
        }
    }

    /// Agent 可执行的下一步建议
    pub fn suggestion(&self) -> &'static str {
        match self {
            ErrorCode::AuthExpired => {
                "重新生成/更新 PAT Token 后重试: atlassian-cli config set <module> --stdin (或用对应环境变量覆盖)"
            }
            ErrorCode::PermissionDenied => {
                "确认当前 Token 对该资源有访问权限,或联系管理员调整项目/空间/仓库权限"
            }
            ErrorCode::NotFound => {
                "核对资源 Key/ID/URL 与 Base URL 前缀 (如 /jira、/confluence),或用 search/list 命令先查找"
            }
            ErrorCode::ParamInvalid => {
                "对照 atlassian-cli <command> --help 检查参数是否缺失或格式错误"
            }
            ErrorCode::ConfigMissing => {
                "运行 atlassian-cli login 完成配置,或设置环境变量 (如 JIRA_URL/JIRA_TOKEN)"
            }
            ErrorCode::HttpError => {
                "检查网络连通性与服务端状态,稍后重试;必要时加 -k/--insecure 信任自签名证书"
            }
            ErrorCode::Generic => "查看 message/detail 定位问题;必要时重试或反馈维护者",
        }
    }
}

/// 结构化错误内部数据(Box 化以将 AppError 栈大小控制在 16 字节内)
#[derive(Debug)]
struct AppErrorInner {
    /// 面向人类的主消息
    message: String,
    /// 服务器原始详情(已清洗,可选)
    detail: Option<String>,
    /// 模块名(由 run() 附加,如 "jira")
    module: Option<String>,
    /// 覆盖默认 suggestion(可选,用于给具体场景更精准的下一步建议)
    suggestion: Option<String>,
    /// 直接可重试执行的完整命令(供 AI Agent 零推理开销执行)
    suggested_command: Option<String>,
    source: Option<anyhow::Error>,
}

impl Clone for AppErrorInner {
    fn clone(&self) -> Self {
        Self {
            message: self.message.clone(),
            detail: self.detail.clone(),
            module: self.module.clone(),
            suggestion: self.suggestion.clone(),
            suggested_command: self.suggested_command.clone(),
            source: None,
        }
    }
}

/// 结构化错误(公共边界统一返回)
#[derive(Debug)]
pub struct AppError {
    pub code: ErrorCode,
    inner: Box<AppErrorInner>,
}

impl AppError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            inner: Box::new(AppErrorInner {
                message: message.into(),
                detail: None,
                module: None,
                suggestion: None,
                suggested_command: None,
                source: None,
            }),
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.inner.detail = Some(detail.into());
        self
    }

    /// 覆盖默认 suggestion(agent 可执行的下一步)
    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.inner.suggestion = Some(suggestion.into());
        self
    }

    /// 提供精准拼接好的完整重试命令行
    pub fn with_suggested_command(mut self, command: impl Into<String>) -> Self {
        self.inner.suggested_command = Some(command.into());
        self
    }

    /// 附加模块前缀(仅首次附加)
    pub fn with_module(mut self, module: &str) -> Self {
        if self.inner.module.is_none() {
            self.inner.module = Some(module.to_string());
        }
        self
    }

    #[allow(dead_code)]
    pub fn message(&self) -> &str {
        &self.inner.message
    }

    #[allow(dead_code)]
    pub fn detail(&self) -> Option<&str> {
        self.inner.detail.as_deref()
    }

    #[allow(dead_code)]
    pub fn module(&self) -> Option<&str> {
        self.inner.module.as_deref()
    }

    #[allow(dead_code)]
    pub fn suggested_command(&self) -> Option<&str> {
        self.inner.suggested_command.as_deref()
    }

    // ---- 语义化构造器 ----
    pub fn auth_expired(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::AuthExpired, msg)
    }
    pub fn permission_denied(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::PermissionDenied, msg)
    }
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::NotFound, msg)
    }
    pub fn param_invalid(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::ParamInvalid, msg)
    }
    pub fn config_missing(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::ConfigMissing, msg)
    }
    pub fn http_error(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::HttpError, msg)
    }
    pub fn generic(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::Generic, msg)
    }

    /// 输出错误 JSON(供 main.rs 收口)
    pub fn to_json(&self) -> serde_json::Value {
        let mut v = serde_json::json!({
            "status": "error",
            "code": self.code.as_str(),
            "message": self.inner.message,
            "suggestion": self.inner.suggestion.clone().unwrap_or_else(|| self.code.suggestion().to_string()),
        });
        if let Some(cmd) = &self.inner.suggested_command {
            v["suggested_command"] = serde_json::json!(cmd);
        }
        if let Some(d) = &self.inner.detail {
            v["detail"] = serde_json::json!(d);
        }
        if let Some(m) = &self.inner.module {
            v["module"] = serde_json::json!(m);
        }
        v
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.inner.module {
            Some(m) => write!(f, "[{}] {}", m, self.inner.message),
            None => write!(f, "{}", self.inner.message),
        }
    }
}

impl std::error::Error for AppError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.inner
            .source
            .as_ref()
            .map(|e| e.as_ref() as &(dyn std::error::Error + 'static))
    }
}

impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        // 若 anyhow 里包的正是 AppError(经 anyhow 中转再取回),恢复其结构化语义
        if let Some(ae) = e.downcast_ref::<AppError>() {
            return Self {
                code: ae.code,
                inner: ae.inner.clone(),
            };
        }
        Self::generic(e.to_string())
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        Self::generic(e.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        Self::http_error(format!("JSON 解析失败: {}", e))
    }
}

impl From<reqwest::Error> for AppError {
    fn from(e: reqwest::Error) -> Self {
        Self::http_error(e.to_string())
    }
}

impl From<reqwest_middleware::Error> for AppError {
    fn from(e: reqwest_middleware::Error) -> Self {
        Self::http_error(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_code_table() {
        // 表驱动:code 字符串 / 退出码 / suggestion 非空
        for code in [
            ErrorCode::AuthExpired,
            ErrorCode::PermissionDenied,
            ErrorCode::NotFound,
            ErrorCode::ParamInvalid,
            ErrorCode::ConfigMissing,
            ErrorCode::HttpError,
            ErrorCode::Generic,
        ] {
            assert!(!code.as_str().is_empty());
            assert!(code.exit_code() > 0);
            assert!(!code.suggestion().is_empty());
        }
        assert_eq!(ErrorCode::AuthExpired.as_str(), "AUTH_EXPIRED");
        assert_eq!(ErrorCode::AuthExpired.exit_code(), 10);
        assert_eq!(ErrorCode::PermissionDenied.exit_code(), 11);
        assert_eq!(ErrorCode::NotFound.exit_code(), 20);
        assert_eq!(ErrorCode::ParamInvalid.exit_code(), 2);
        assert_eq!(ErrorCode::ConfigMissing.exit_code(), 3);
        assert_eq!(ErrorCode::HttpError.exit_code(), 1);
    }

    #[test]
    fn test_app_error_display_and_json() {
        let e = AppError::auth_expired("认证失败: PAT Token 无效或已过期")
            .with_detail("HTTP [401] Basic auth failure")
            .with_module("jira");
        assert_eq!(e.to_string(), "[jira] 认证失败: PAT Token 无效或已过期");

        let json = e.to_json();
        assert_eq!(json["status"], "error");
        assert_eq!(json["code"], "AUTH_EXPIRED");
        assert_eq!(json["module"], "jira");
        assert_eq!(json["detail"], "HTTP [401] Basic auth failure");
        assert!(!json["suggestion"].as_str().unwrap().is_empty());
    }

    #[test]
    fn test_suggestion_override() {
        let e = AppError::param_invalid("写操作需要显式确认")
            .with_suggestion("确认执行请追加 --confirm;仅预览请追加 --dry-run");
        let json = e.to_json();
        assert_eq!(
            json["suggestion"],
            "确认执行请追加 --confirm;仅预览请追加 --dry-run"
        );

        // 无 override 时回退到 code 默认建议
        let e2 = AppError::param_invalid("x");
        assert!(e2.to_json()["suggestion"].as_str().unwrap().contains("--help"));
    }

    #[test]
    fn test_with_module_only_once() {
        let e = AppError::not_found("x").with_module("jira").with_module("bitbucket");
        assert_eq!(e.module(), Some("jira"));
    }

    #[test]
    fn test_from_anyhow_recovers_app_error() {
        let original = AppError::permission_denied("权限拒绝");
        let anyhow_err: anyhow::Error = anyhow::Error::new(original);
        let back: AppError = anyhow_err.into();
        assert_eq!(back.code, ErrorCode::PermissionDenied);
        assert_eq!(back.message(), "权限拒绝");
    }

    #[test]
    fn test_from_plain_anyhow_maps_generic() {
        let e: AppError = anyhow::anyhow!("something exploded").into();
        assert_eq!(e.code, ErrorCode::Generic);
        assert_eq!(e.code.exit_code(), 1);
    }
}
