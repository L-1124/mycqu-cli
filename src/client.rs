//! HTTP 客户端构造与上游错误到输出契约错误码的映射。

use rsmycqu::errors::session::SessionError;
use rsmycqu::errors::{ApiError, RsMyCQUError};
use rsmycqu::errors::sso::SSOError;
use rsmycqu::session::Client;
use std::time::Duration;

use crate::output;

/// 统一配置的 HTTP 客户端：连接超时 10 秒，总请求超时 30 秒。
///
/// 构建失败时返回错误，由调用方终止命令；绝不静默回退到不带超时配置的
/// 默认客户端，避免调用方无法确认实际生效的超时设置。
pub fn build_client() -> Result<Client, String> {
    Client::custom(|builder| {
        builder
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
    })
    .map_err(|e| format!("Failed to build HTTP client with timeout configuration: {e}"))
}

/// 映射后的错误：退出码、JSON 错误码、消息与是否适合自动重试。
pub struct MappedError {
    pub exit_code: i32,
    pub code: &'static str,
    pub message: String,
    pub retryable: bool,
}

impl MappedError {
    /// 按输出契约把错误写入 stdout。
    pub fn report(&self) {
        output::print_err(self.code, &self.message, self.retryable, None, None);
    }

    /// 标记为“结果不确定的副作用请求”：禁止自动重试。
    ///
    /// 网络超时或传输失败不代表服务端没有执行动作（验证码可能已发出、
    /// 会话可能已退出），调用方必须先确认当前状态再决定是否重试。
    /// 已经不可重试的错误保持原有消息，不追加提示。
    pub fn unconfirmed_side_effect(mut self) -> Self {
        if self.retryable {
            self.retryable = false;
            self.message = format!(
                "{} (the request may have reached the server; confirm the current state before retrying)",
                self.message
            );
        }
        self
    }
}

/// 构建统一配置的客户端；构建失败时按输出契约报告并返回退出码。
pub fn client_or_report() -> Result<Client, i32> {
    build_client().map_err(|e| {
        output::print_err(
            "UPSTREAM_ERROR",
            &format!("Failed to initialize HTTP client: {e}"),
            false,
            None,
            None,
        );
        output::EXIT_UPSTREAM_ERROR
    })
}

/// 映射上游错误并立即报告，返回退出码。
pub fn report_api_error<T: RsMyCQUError>(context: &str, err: &ApiError<T>) -> i32 {
    let mapped = map_api_error(context, err);
    mapped.report();
    mapped.exit_code
}

/// 映射 SSO 错误并立即报告，返回退出码。
pub fn report_sso_error(context: &str, err: &ApiError<SSOError>) -> i32 {
    let mapped = map_sso_error(context, err);
    mapped.report();
    mapped.exit_code
}

/// 按底层可区分的错误类型映射 [`ApiError`]。
///
/// 网络故障、上游异常、响应结构错误与权限问题必须可区分；无法区分时按
/// 上游异常处理，不武断地认定为权限错误。
pub fn map_api_error<T: RsMyCQUError>(context: &str, err: &ApiError<T>) -> MappedError {
    use rsmycqu::errors::ApiError as E;

    let (code, exit_code, retryable) = match err {
        E::NotLogin => (
            "AUTH_REQUIRED",
            crate::output::EXIT_AUTH_REQUIRED,
            false,
        ),
        E::NotAccess => (
            "PERMISSION_DENIED",
            crate::output::EXIT_PERMISSION_DENIED,
            false,
        ),
        E::Request { .. } if err.is_timeout() => (
            "NETWORK_TIMEOUT",
            crate::output::EXIT_UPSTREAM_ERROR,
            true,
        ),
        E::Request { .. } => (
            "UPSTREAM_ERROR",
            crate::output::EXIT_UPSTREAM_ERROR,
            true,
        ),
        E::HttpStatus { status } => match status.as_u16() {
            401 | 403 => (
                "PERMISSION_DENIED",
                crate::output::EXIT_PERMISSION_DENIED,
                false,
            ),
            408 | 429 | 500 | 502 | 503 | 504 => (
                "UPSTREAM_ERROR",
                crate::output::EXIT_UPSTREAM_ERROR,
                true,
            ),
            _ => (
                "UPSTREAM_ERROR",
                crate::output::EXIT_UPSTREAM_ERROR,
                false,
            ),
        },
        E::ModelParse { .. } => (
            "RESPONSE_INVALID",
            crate::output::EXIT_UPSTREAM_ERROR,
            false,
        ),
        // 瑞数是上游防护状态，不是本地状态损坏
        E::Session {
            source: SessionError::RuishuError { .. },
        } => (
            "UPSTREAM_ERROR",
            crate::output::EXIT_UPSTREAM_ERROR,
            false,
        ),
        E::Session { .. } => (
            "STATE_INVALID",
            crate::output::EXIT_STATE_INVALID,
            false,
        ),
        E::Website { .. } | E::Inner { .. } | E::Whatever { .. } => (
            "UPSTREAM_ERROR",
            crate::output::EXIT_UPSTREAM_ERROR,
            false,
        ),
    };

    MappedError {
        exit_code,
        code,
        message: format!("{context}: {err}"),
        retryable,
    }
}

/// SSO 待完成认证流程相关错误的细化映射。
///
/// 流程过期、失效、未开始、阶段不匹配与通道未绑定都是可恢复的认证状态，
/// 不应统一报成上游异常。
pub fn map_sso_error(context: &str, err: &ApiError<SSOError>) -> MappedError {
    use rsmycqu::errors::ApiError as E;

    if let E::Inner { source } = err {
        match source {
            SSOError::PendingLoginExpired | SSOError::PendingLoginInvalidated { .. } => {
                return MappedError {
                    exit_code: crate::output::EXIT_AUTH_REQUIRED,
                    code: "AUTH_FLOW_EXPIRED",
                    message: format!(
                        "{context}: pending authentication flow expired or was invalidated; restart with 'auth login'"
                    ),
                    retryable: false,
                };
            }
            SSOError::PendingLoginNotStarted => {
                return MappedError {
                    exit_code: crate::output::EXIT_AUTH_REQUIRED,
                    code: "AUTH_REQUIRED",
                    message: format!("{context}: no pending authentication flow in this session"),
                    retryable: false,
                };
            }
            SSOError::InvalidPendingState { .. } => {
                return MappedError {
                    exit_code: crate::output::EXIT_AUTH_REQUIRED,
                    code: "AUTH_REQUIRED",
                    message: format!("{context}: {err}"),
                    retryable: false,
                };
            }
            SSOError::NoSecondFactorBindings
            | SSOError::PhoneNotBound
            | SSOError::EmailNotBound => {
                return MappedError {
                    exit_code: crate::output::EXIT_INVALID_ARGUMENT,
                    code: "INVALID_ARGUMENT",
                    message: format!(
                        "{context}: no contact bound for the selected second-factor channel; try the other --kind"
                    ),
                    retryable: false,
                };
            }
            SSOError::SecondFactorError { .. } => {
                return MappedError {
                    exit_code: crate::output::EXIT_UPSTREAM_ERROR,
                    code: "UPSTREAM_ERROR",
                    message: format!("{context}: {err}"),
                    retryable: false,
                };
            }
            SSOError::LogoutError => {
                return MappedError {
                    exit_code: crate::output::EXIT_UPSTREAM_ERROR,
                    code: "UPSTREAM_ERROR",
                    message: format!("{context}: {err}"),
                    retryable: true,
                };
            }
            SSOError::RuishuError { .. } => {
                return MappedError {
                    exit_code: crate::output::EXIT_UPSTREAM_ERROR,
                    code: "UPSTREAM_ERROR",
                    message: format!("{context}: {err}"),
                    retryable: true,
                };
            }
            _ => {}
        }
    }

    map_api_error(context, err)
}
