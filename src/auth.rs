use clap::{Args, Subcommand};
use rsmycqu::errors::ApiError;
use rsmycqu::errors::sso::SSOError;
use rsmycqu::session::Session;
use rsmycqu::sso::status::SsoSessionStatus;
use rsmycqu::sso::{
    PendingLoginStatus, SecondFactorKind, cancel_pending_login, check_sso_session,
    pending_captcha_image, pending_login_status, resume_login, send_pending_second_factor_code,
    start_login, submit_pending_second_factor,
};
use serde::Serialize;
use std::io::{self, BufRead};

use crate::challenge;
use crate::client::{client_or_report, map_sso_error, report_sso_error};
use crate::output::{self, ApiResponse, EXIT_AUTH_REQUIRED, EXIT_SUCCESS};
use crate::store::{self, StateStore};

#[derive(Subcommand)]
pub enum AuthCommands {
    #[command(about = "登录统一身份认证账号")]
    Login(LoginArgs),
    #[command(about = "检查当前会话状态（支持在线探测）")]
    Status(StatusArgs),
    #[command(about = "获取当前待完成认证的图形验证码图片")]
    Captcha,
    #[command(about = "发送或重发二次身份验证码")]
    SendCode(SendCodeArgs),
    #[command(about = "提交图形验证码或二次验证码继续登录")]
    Verify(VerifyArgs),
    #[command(about = "退出当前登录并清除本地会话")]
    Logout,
}

#[derive(Args)]
pub struct LoginArgs {
    #[arg(short, long, help = "统一身份认证账号（学工号）")]
    pub auth: String,
    #[arg(long, help = "密码通过 stdin 读取")]
    pub stdin_password: bool,
}

#[derive(Args)]
pub struct StatusArgs {
    #[arg(long, help = "发起在线网络请求确证 SSO 有效性")]
    pub online: bool,
}

#[derive(Args)]
pub struct SendCodeArgs {
    #[arg(
        long,
        value_enum,
        default_value = "sms",
        help = "验证码通道类型: sms 或 mail"
    )]
    pub kind: FactorKindArg,
}

#[derive(clap::ValueEnum, Clone, Copy)]
pub enum FactorKindArg {
    Sms,
    Mail,
}

impl From<FactorKindArg> for SecondFactorKind {
    fn from(val: FactorKindArg) -> Self {
        match val {
            FactorKindArg::Sms => SecondFactorKind::Sms,
            FactorKindArg::Mail => SecondFactorKind::Mail,
        }
    }
}

#[derive(Args)]
pub struct VerifyArgs {
    #[arg(
        long,
        help = "待续接的认证流程挑战 ID（必填，取自上一次认证响应的 challenge_id）"
    )]
    pub challenge_id: String,
    #[arg(long, help = "从标准输入 (stdin) 读取验证码内容")]
    pub stdin_code: bool,
    #[arg(long, value_enum, default_value = "sms", help = "二次验证通道类型")]
    pub kind: FactorKindArg,
    #[arg(long, help = "图形验证码重试时通过 stdin 输入密码")]
    pub stdin_password: bool,
}

#[derive(Serialize)]
pub struct AuthStatusOutput {
    pub is_login: bool,
    pub pending_stage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub online_status: Option<String>,
}

/// 本地清理结果。
///
/// 认证失败时主错误不应被清理问题掩盖，但调用方必须能看出流程是否真的
/// 已经终止、本地凭据是否确实清理干净。
#[derive(Serialize)]
struct CleanupStatus {
    /// 会话终态是否已写入本地
    #[serde(skip_serializing_if = "Option::is_none")]
    session_persisted: Option<bool>,
    /// 本地挑战状态是否已清理（流程结束时为 true 才表示流程真正结束）
    #[serde(skip_serializing_if = "Option::is_none")]
    challenge_cleared: Option<bool>,
    /// 验证码图片是否已清理
    #[serde(skip_serializing_if = "Option::is_none")]
    captcha_cleared: Option<bool>,
}

impl CleanupStatus {
    /// 流程结束：清理挑战与验证码，记录三项结果。
    fn terminal(store: &StateStore, session_persisted: bool) -> Self {
        Self {
            session_persisted: Some(session_persisted),
            challenge_cleared: Some(store.clear_challenge().is_ok()),
            captcha_cleared: Some(store.clear_captcha().is_ok()),
        }
    }

    /// 流程继续（阶段转换、挑战轮换）：只报告验证码清理结果。
    fn continuing(store: &StateStore) -> Self {
        Self {
            session_persisted: None,
            challenge_cleared: None,
            captcha_cleared: Some(store.clear_captcha().is_ok()),
        }
    }
}

fn read_secret_input(stdin_mode: bool, prompt: &str) -> Result<String, String> {
    if stdin_mode {
        let mut line = String::new();
        io::stdin()
            .lock()
            .read_line(&mut line)
            .map_err(|e| format!("Failed to read input from stdin: {e}"))?;
        // 只移除行末换行符，保留密码中可能有意的尾随空格等字符
        let without_newline = line.strip_suffix('\n').unwrap_or(&line);
        let trimmed = without_newline
            .strip_suffix('\r')
            .unwrap_or(without_newline);
        // 部分 shell/管道会附带 UTF-8 BOM，它不属于凭据内容
        let without_bom = trimmed.strip_prefix('\u{feff}').unwrap_or(trimmed);
        if without_bom.is_empty() {
            return Err("Input from stdin cannot be empty".to_string());
        }
        Ok(without_bom.to_string())
    } else {
        rpassword::prompt_password(prompt).map_err(|e| format!("Failed to prompt password: {e}"))
    }
}

/// 认证流程已终结（过期或被库标记失效）：持久化终态并清理挑战与验证码。
fn finalize_dead_flow(store: &StateStore, session: &Session) -> CleanupStatus {
    let persisted = store.save_session(session).is_ok();
    CleanupStatus::terminal(store, persisted)
}

/// 报告认证错误，并附带本地清理结果。
fn report_flow_error(
    code: &'static str,
    message: &str,
    retryable: bool,
    exit_code: i32,
    cleanup: CleanupStatus,
) -> i32 {
    let mut resp = ApiResponse::<serde_json::Value>::err(code, message, retryable, None, None);
    resp.data = Some(serde_json::json!({ "cleanup": cleanup }));
    output::print_json(&resp);
    exit_code
}

/// 本地待完成认证阶段；`None` 仅表示确实没有待完成流程。
fn pending_stage(session: &Session) -> Result<Option<String>, i32> {
    match pending_login_status(session) {
        Ok(status) => Ok(Some(format!("{status:?}"))),
        // 库只在“没有待完成流程且未登录”时返回该错误，属正常本地状态
        Err(ApiError::Inner {
            source: SSOError::PendingLoginNotStarted,
        }) => Ok(None),
        Err(err) => Err(report_sso_error(
            "Failed to read pending authentication state",
            &err,
        )),
    }
}

async fn handle_login_action(store: &StateStore, args: LoginArgs) -> i32 {
    let password = match read_secret_input(args.stdin_password, "Password: ") {
        Ok(p) => p,
        Err(err) => {
            output::print_err("INVALID_ARGUMENT", &err, false, None, None);
            return output::EXIT_INVALID_ARGUMENT;
        }
    };

    let client = match client_or_report() {
        Ok(client) => client,
        Err(code) => return code,
    };

    let _lock = match store::lock_or_report(store) {
        Ok(lock) => lock,
        Err(code) => return code,
    };

    // 读取旧会话：只有 NotFound 表示“可以登录”，其他错误必须如实上报
    match store.load_session() {
        Ok(existing) if existing.is_login() => {
            output::print_err(
                "AUTH_REQUIRED",
                "A session already exists; run 'auth logout' before logging in again",
                false,
                None,
                None,
            );
            return EXIT_AUTH_REQUIRED;
        }
        Ok(_) => {}
        // 没有会话或旧流程已过期都可以直接重新登录
        Err(store::StoreError::NotFound) | Err(store::StoreError::Expired(_)) => {}
        Err(err) => return store::report_store_error(err),
    }

    let mut session = Session::new();
    match start_login(&client, &mut session, &args.auth, &password).await {
        Ok(PendingLoginStatus::Completed) => {
            if let Err(e) = store.save_session(&session) {
                output::print_err(
                    "STATE_INVALID",
                    &format!("Failed to save session: {e}"),
                    false,
                    None,
                    None,
                );
                return output::EXIT_STATE_INVALID;
            }
            let cleanup = CleanupStatus::terminal(store, true);
            output::print_json(&ApiResponse::success(
                serde_json::json!({ "message": "Login successful", "cleanup": cleanup }),
                true,
                None,
            ));
            EXIT_SUCCESS
        }
        Ok(PendingLoginStatus::CaptchaRequired) => {
            let cid = challenge::generate_challenge_id();
            if let Err(e) = store.save_pending_session(&session, &cid) {
                output::print_err(
                    "STATE_INVALID",
                    &format!("Failed to save pending session: {e}"),
                    false,
                    None,
                    None,
                );
                return output::EXIT_STATE_INVALID;
            }
            output::print_err(
                "CAPTCHA_REQUIRED",
                "Image captcha is required. Call 'auth captcha' to fetch image, then submit code via stdin with 'auth verify --challenge-id <id> --stdin-code'.",
                false,
                Some(cid),
                Some("auth captcha".to_string()),
            );
            EXIT_AUTH_REQUIRED
        }
        Ok(PendingLoginStatus::SecondFactor(sec_status)) => {
            let cid = challenge::generate_challenge_id();
            if let Err(e) = store.save_pending_session(&session, &cid) {
                output::print_err(
                    "STATE_INVALID",
                    &format!("Failed to save pending session: {e}"),
                    false,
                    None,
                    None,
                );
                return output::EXIT_STATE_INVALID;
            }
            let cleanup = CleanupStatus::continuing(store);
            let mut err_resp = ApiResponse::<serde_json::Value>::err(
                "SECOND_FACTOR_REQUIRED",
                "Second factor authentication required. Call 'auth send-code' or submit code with 'auth verify --challenge-id <id> --stdin-code'.",
                false,
                Some(cid),
                Some("auth send-code".to_string()),
            );
            err_resp.data = Some(serde_json::json!({
                "stage": "second_factor",
                "sms_available": sec_status.sms_available,
                "mail_available": sec_status.mail_available,
                "masked_phone": sec_status.masked_phone,
                "masked_email": sec_status.masked_email,
                "cleanup": cleanup,
            }));
            output::print_json(&err_resp);
            EXIT_AUTH_REQUIRED
        }
        Ok(PendingLoginStatus::CredentialRetry) => {
            // 服务端提供了可重试表单：同样签发挑战 ID，供 auth verify 续接
            let cid = challenge::generate_challenge_id();
            if let Err(e) = store.save_pending_session(&session, &cid) {
                output::print_err(
                    "STATE_INVALID",
                    &format!("Failed to save pending retry state: {e}"),
                    false,
                    None,
                    None,
                );
                return output::EXIT_STATE_INVALID;
            }
            output::print_err(
                "INVALID_CREDENTIALS",
                "Incorrect credentials or account rejected; retry with 'auth verify --challenge-id <id> --stdin-password'.",
                false,
                Some(cid),
                Some("auth verify".to_string()),
            );
            EXIT_AUTH_REQUIRED
        }
        Ok(PendingLoginStatus::Expired) | Ok(PendingLoginStatus::Invalidated) => {
            let cleanup = finalize_dead_flow(store, &session);
            report_flow_error(
                "AUTH_FLOW_EXPIRED",
                "Authentication flow expired or was invalidated; restart with 'auth login'",
                false,
                EXIT_AUTH_REQUIRED,
                cleanup,
            )
        }
        // 登录请求属于副作用动作：传输失败时结果不确定，禁止自动重试
        Err(err) => {
            let mapped = map_sso_error("SSO login failed", &err).unconfirmed_side_effect();
            mapped.report();
            mapped.exit_code
        }
    }
}

async fn handle_status_action(store: &StateStore, args: StatusArgs) -> i32 {
    let session = match store.load_session() {
        Ok(session) => session,
        // 无本地会话是合法的“未登录”状态，不是错误
        Err(store::StoreError::NotFound) => {
            output::print_json(&ApiResponse::success(
                AuthStatusOutput {
                    is_login: false,
                    pending_stage: None,
                    online_status: None,
                },
                true,
                None,
            ));
            return EXIT_SUCCESS;
        }
        Err(err) => return store::report_store_error(err),
    };

    let pending = match pending_stage(&session) {
        Ok(pending) => pending,
        Err(code) => return code,
    };

    if !args.online {
        output::print_json(&ApiResponse::success(
            AuthStatusOutput {
                is_login: session.is_login(),
                pending_stage: pending,
                online_status: None,
            },
            true,
            None,
        ));
        return EXIT_SUCCESS;
    }

    // 只有在线探测才需要 HTTP 客户端
    let client = match client_or_report() {
        Ok(client) => client,
        Err(code) => return code,
    };
    let now_str = chrono::Utc::now().to_rfc3339();

    match check_sso_session(&client, &session).await {
        Ok(SsoSessionStatus::Authenticated) => {
            let mut resp = ApiResponse::success(
                AuthStatusOutput {
                    is_login: true,
                    pending_stage: pending,
                    online_status: Some("Authenticated".to_string()),
                },
                true,
                None,
            );
            resp.meta.checked_service = Some("SSO (sso.cqu.edu.cn)".to_string());
            resp.meta.checked_at = Some(now_str);
            output::print_json(&resp);
            EXIT_SUCCESS
        }
        Ok(SsoSessionStatus::Unauthenticated) => {
            let mut resp = ApiResponse::success(
                AuthStatusOutput {
                    is_login: false,
                    pending_stage: pending,
                    online_status: Some("Unauthenticated".to_string()),
                },
                true,
                None,
            );
            resp.meta.checked_service = Some("SSO (sso.cqu.edu.cn)".to_string());
            resp.meta.checked_at = Some(now_str);
            output::print_json(&resp);
            EXIT_SUCCESS
        }
        Err(e) => {
            let mapped = map_sso_error("Failed to perform online status check", &e);
            let mut resp = ApiResponse::<serde_json::Value>::err(
                mapped.code,
                &mapped.message,
                mapped.retryable,
                None,
                None,
            );
            resp.meta.checked_service = Some("SSO (sso.cqu.edu.cn)".to_string());
            resp.meta.checked_at = Some(now_str);
            output::print_json(&resp);
            mapped.exit_code
        }
    }
}

async fn handle_captcha_action(store: &StateStore) -> i32 {
    let client = match client_or_report() {
        Ok(client) => client,
        Err(code) => return code,
    };
    let session = match store::load_session_or_report(store) {
        Ok(session) => session,
        Err(code) => return code,
    };

    match pending_captcha_image(&client, &session).await {
        Ok(bytes) => match store.save_captcha(&bytes) {
            Ok(path) => {
                output::print_json(&ApiResponse::success(
                    serde_json::json!({
                        "captcha_path": path.to_string_lossy(),
                        "format": "image/png"
                    }),
                    true,
                    None,
                ));
                EXIT_SUCCESS
            }
            Err(e) => {
                output::print_err(
                    "STATE_INVALID",
                    &format!("Failed to save captcha image: {e}"),
                    false,
                    None,
                    None,
                );
                output::EXIT_STATE_INVALID
            }
        },
        Err(e) => report_sso_error("Failed to download captcha image", &e),
    }
}

async fn handle_send_code_action(store: &StateStore, args: SendCodeArgs) -> i32 {
    let client = match client_or_report() {
        Ok(client) => client,
        Err(code) => return code,
    };
    let session = match store::load_session_or_report(store) {
        Ok(session) => session,
        Err(code) => return code,
    };

    match send_pending_second_factor_code(&client, &session, args.kind.into()).await {
        Ok(()) => {
            output::print_json(&ApiResponse::success(
                serde_json::json!({ "message": "Verification code sent" }),
                true,
                None,
            ));
            EXIT_SUCCESS
        }
        // 发送验证码是副作用动作：超时不代表未发送，禁止自动重试
        Err(err) => {
            let mapped =
                map_sso_error("Failed to send verification code", &err).unconfirmed_side_effect();
            mapped.report();
            mapped.exit_code
        }
    }
}

async fn handle_verify_action(store: &StateStore, args: VerifyArgs) -> i32 {
    let client = match client_or_report() {
        Ok(client) => client,
        Err(code) => return code,
    };

    // 状态锁覆盖整个“读取-修改-保存”临界区，避免并发进程互相覆盖
    let _lock = match store::lock_or_report(store) {
        Ok(lock) => lock,
        Err(code) => return code,
    };

    let mut session = match store::load_session_or_report(store) {
        Ok(session) => session,
        Err(code) => return code,
    };

    let status = match pending_login_status(&session) {
        Ok(status) => status,
        Err(err) => return report_sso_error("No pending login stage", &err),
    };

    // 终态流程直接拒绝，不必进入挑战核对
    match status {
        PendingLoginStatus::Completed => {
            output::print_err(
                "AUTH_REQUIRED",
                "Authentication already completed; no verification is pending",
                false,
                None,
                None,
            );
            return EXIT_AUTH_REQUIRED;
        }
        PendingLoginStatus::Expired | PendingLoginStatus::Invalidated => {
            let cleanup = CleanupStatus::terminal(store, true);
            return report_flow_error(
                "AUTH_FLOW_EXPIRED",
                "Pending authentication flow expired or was invalidated; restart with 'auth login'",
                false,
                EXIT_AUTH_REQUIRED,
                cleanup,
            );
        }
        _ => {}
    }

    // 在读取验证码、发起网络请求之前，先确认挑战 ID 对应当前待完成流程
    let stored = match store.load_challenge() {
        Ok(stored) => stored,
        // 文件存在但无法读取或解析：本地状态损坏，不是认证超时
        Err(msg) => {
            output::print_err(
                "STATE_INVALID",
                &format!("Challenge state is unreadable: {msg}"),
                false,
                None,
                None,
            );
            return output::EXIT_STATE_INVALID;
        }
    };
    match challenge::check_challenge(stored.as_ref(), &args.challenge_id, challenge::now_unix()) {
        challenge::ChallengeCheck::Current => {}
        challenge::ChallengeCheck::Expired => {
            let cleanup = CleanupStatus::terminal(store, true);
            return report_flow_error(
                "AUTH_FLOW_EXPIRED",
                "Pending authentication flow expired; restart with 'auth login'",
                false,
                EXIT_AUTH_REQUIRED,
                cleanup,
            );
        }
        challenge::ChallengeCheck::Missing => {
            return report_flow_error(
                "AUTH_FLOW_EXPIRED",
                "No persisted challenge for the pending flow; restart with 'auth login'",
                false,
                EXIT_AUTH_REQUIRED,
                CleanupStatus::continuing(store),
            );
        }
        challenge::ChallengeCheck::Mismatch => {
            // 流程本身仍然有效，只是 ID 不对：不清除任何本地状态
            output::print_err(
                "AUTH_FLOW_EXPIRED",
                "challenge_id does not match the pending authentication flow; use the challenge_id returned by the latest auth response",
                false,
                None,
                None,
            );
            return EXIT_AUTH_REQUIRED;
        }
    }

    let resume_res = match status {
        PendingLoginStatus::SecondFactor(_) => {
            let input_code = match read_secret_input(args.stdin_code, "Verification Code: ") {
                Ok(code) => code,
                Err(err) => {
                    output::print_err("INVALID_ARGUMENT", &err, false, None, None);
                    return output::EXIT_INVALID_ARGUMENT;
                }
            };
            return match submit_pending_second_factor(
                &client,
                &mut session,
                args.kind.into(),
                input_code,
            )
            .await
            {
                Ok(PendingLoginStatus::Completed) => {
                    if let Err(e) = store.save_session(&session) {
                        output::print_err(
                            "STATE_INVALID",
                            &format!("Failed to save session: {e}"),
                            false,
                            None,
                            None,
                        );
                        return output::EXIT_STATE_INVALID;
                    }
                    let cleanup = CleanupStatus::terminal(store, true);
                    output::print_json(&ApiResponse::success(
                        serde_json::json!({ "message": "Verification successful", "cleanup": cleanup }),
                        true,
                        None,
                    ));
                    EXIT_SUCCESS
                }
                Ok(PendingLoginStatus::SecondFactor(sec_status)) => {
                    // 阶段延续：轮换挑战 ID，避免无条件复用旧 ID
                    let cid = challenge::generate_challenge_id();
                    if let Err(e) = store.save_pending_session(&session, &cid) {
                        output::print_err(
                            "STATE_INVALID",
                            &format!("Failed to save pending session: {e}"),
                            false,
                            None,
                            None,
                        );
                        return output::EXIT_STATE_INVALID;
                    }
                    let cleanup = CleanupStatus::continuing(store);
                    let mut err_resp = ApiResponse::<serde_json::Value>::err(
                        "SECOND_FACTOR_REQUIRED",
                        "Second factor verification still required. Call 'auth send-code' or submit code with 'auth verify --challenge-id <id> --stdin-code'.",
                        false,
                        Some(cid),
                        Some("auth send-code".to_string()),
                    );
                    err_resp.data = Some(serde_json::json!({
                        "stage": "second_factor",
                        "sms_available": sec_status.sms_available,
                        "mail_available": sec_status.mail_available,
                        "masked_phone": sec_status.masked_phone,
                        "masked_email": sec_status.masked_email,
                        "cleanup": cleanup,
                    }));
                    output::print_json(&err_resp);
                    EXIT_AUTH_REQUIRED
                }
                Ok(PendingLoginStatus::CaptchaRequired) => {
                    let cid = challenge::generate_challenge_id();
                    if let Err(e) = store.save_pending_session(&session, &cid) {
                        output::print_err(
                            "STATE_INVALID",
                            &format!("Failed to save pending session: {e}"),
                            false,
                            None,
                            None,
                        );
                        return output::EXIT_STATE_INVALID;
                    }
                    let cleanup = CleanupStatus::continuing(store);
                    let mut err_resp = ApiResponse::<serde_json::Value>::err(
                        "CAPTCHA_REQUIRED",
                        "Captcha required; fetch captcha with 'auth captcha' and submit with 'auth verify --challenge-id <id> --stdin-code'.",
                        false,
                        Some(cid),
                        Some("auth captcha".to_string()),
                    );
                    err_resp.data = Some(serde_json::json!({ "cleanup": cleanup }));
                    output::print_json(&err_resp);
                    EXIT_AUTH_REQUIRED
                }
                Ok(PendingLoginStatus::CredentialRetry) => {
                    let cid = challenge::generate_challenge_id();
                    if let Err(e) = store.save_pending_session(&session, &cid) {
                        output::print_err(
                            "STATE_INVALID",
                            &format!("Failed to save pending session: {e}"),
                            false,
                            None,
                            None,
                        );
                        return output::EXIT_STATE_INVALID;
                    }
                    let cleanup = CleanupStatus::continuing(store);
                    let mut err_resp = ApiResponse::<serde_json::Value>::err(
                        "INVALID_CREDENTIALS",
                        "Credentials rejected; retry password with 'auth verify --challenge-id <id> --stdin-password'.",
                        false,
                        Some(cid),
                        Some("auth verify".to_string()),
                    );
                    err_resp.data = Some(serde_json::json!({ "cleanup": cleanup }));
                    output::print_json(&err_resp);
                    EXIT_AUTH_REQUIRED
                }
                Ok(PendingLoginStatus::Expired) | Ok(PendingLoginStatus::Invalidated) => {
                    let cleanup = finalize_dead_flow(store, &session);
                    report_flow_error(
                        "AUTH_FLOW_EXPIRED",
                        "Authentication flow expired or was invalidated; restart with 'auth login'",
                        false,
                        EXIT_AUTH_REQUIRED,
                        cleanup,
                    )
                }
                // 提交验证码是副作用动作：传输失败时结果不确定，禁止自动重试
                Err(err) => {
                    let cleanup = finalize_dead_flow(store, &session);
                    let mapped = map_sso_error("Failed to submit verification code", &err)
                        .unconfirmed_side_effect();
                    report_flow_error(
                        mapped.code,
                        &mapped.message,
                        mapped.retryable,
                        mapped.exit_code,
                        cleanup,
                    )
                }
            };
        }
        PendingLoginStatus::CaptchaRequired => {
            let input_code = match read_secret_input(args.stdin_code, "Captcha Code: ") {
                Ok(code) => code,
                Err(err) => {
                    output::print_err("INVALID_ARGUMENT", &err, false, None, None);
                    return output::EXIT_INVALID_ARGUMENT;
                }
            };
            let password = match read_secret_input(args.stdin_password, "Re-enter Password: ") {
                Ok(p) => p,
                Err(err) => {
                    output::print_err("INVALID_ARGUMENT", &err, false, None, None);
                    return output::EXIT_INVALID_ARGUMENT;
                }
            };
            resume_login(&client, &mut session, password, Some(&input_code)).await
        }
        PendingLoginStatus::CredentialRetry => {
            let password = match read_secret_input(args.stdin_password, "Password: ") {
                Ok(p) => p,
                Err(err) => {
                    output::print_err("INVALID_ARGUMENT", &err, false, None, None);
                    return output::EXIT_INVALID_ARGUMENT;
                }
            };
            resume_login(&client, &mut session, password, None).await
        }
        other => {
            return report_flow_error(
                "AUTH_FLOW_EXPIRED",
                &format!(
                    "Authentication flow is no longer active ({other:?}); restart with 'auth login'"
                ),
                false,
                EXIT_AUTH_REQUIRED,
                CleanupStatus::continuing(store),
            );
        }
    };

    match resume_res {
        Ok(PendingLoginStatus::Completed) => {
            if let Err(e) = store.save_session(&session) {
                output::print_err(
                    "STATE_INVALID",
                    &format!("Failed to save session: {e}"),
                    false,
                    None,
                    None,
                );
                return output::EXIT_STATE_INVALID;
            }
            let cleanup = CleanupStatus::terminal(store, true);
            output::print_json(&ApiResponse::success(
                serde_json::json!({ "message": "Login successful", "cleanup": cleanup }),
                true,
                None,
            ));
            EXIT_SUCCESS
        }
        Ok(PendingLoginStatus::SecondFactor(sec_status)) => {
            // 图形验证码通过后转入二次验证：签发全新挑战 ID，
            // 绝不复用旧 ID，也不使用固定 ID
            let cid = challenge::generate_challenge_id();
            if let Err(e) = store.save_pending_session(&session, &cid) {
                output::print_err(
                    "STATE_INVALID",
                    &format!("Failed to save pending session: {e}"),
                    false,
                    None,
                    None,
                );
                return output::EXIT_STATE_INVALID;
            }
            let cleanup = CleanupStatus::continuing(store);
            let mut err_resp = ApiResponse::<serde_json::Value>::err(
                "SECOND_FACTOR_REQUIRED",
                "Second factor verification required. Call 'auth send-code' or submit code with 'auth verify --challenge-id <id> --stdin-code'.",
                false,
                Some(cid),
                Some("auth send-code".to_string()),
            );
            err_resp.data = Some(serde_json::json!({
                "stage": "second_factor",
                "sms_available": sec_status.sms_available,
                "mail_available": sec_status.mail_available,
                "masked_phone": sec_status.masked_phone,
                "masked_email": sec_status.masked_email,
                "cleanup": cleanup,
            }));
            output::print_json(&err_resp);
            EXIT_AUTH_REQUIRED
        }
        Ok(PendingLoginStatus::CaptchaRequired) => {
            let cid = challenge::generate_challenge_id();
            if let Err(e) = store.save_pending_session(&session, &cid) {
                output::print_err(
                    "STATE_INVALID",
                    &format!("Failed to save pending session: {e}"),
                    false,
                    None,
                    None,
                );
                return output::EXIT_STATE_INVALID;
            }
            let cleanup = CleanupStatus::continuing(store);
            let mut err_resp = ApiResponse::<serde_json::Value>::err(
                "CAPTCHA_REQUIRED",
                "Captcha required; fetch captcha with 'auth captcha' and submit with 'auth verify --challenge-id <id> --stdin-code'.",
                false,
                Some(cid),
                Some("auth captcha".to_string()),
            );
            err_resp.data = Some(serde_json::json!({ "cleanup": cleanup }));
            output::print_json(&err_resp);
            EXIT_AUTH_REQUIRED
        }
        Ok(PendingLoginStatus::CredentialRetry) => {
            let cid = challenge::generate_challenge_id();
            if let Err(e) = store.save_pending_session(&session, &cid) {
                output::print_err(
                    "STATE_INVALID",
                    &format!("Failed to save pending session: {e}"),
                    false,
                    None,
                    None,
                );
                return output::EXIT_STATE_INVALID;
            }
            let cleanup = CleanupStatus::continuing(store);
            let mut err_resp = ApiResponse::<serde_json::Value>::err(
                "INVALID_CREDENTIALS",
                "Credentials rejected; retry password with 'auth verify --challenge-id <id> --stdin-password'.",
                false,
                Some(cid),
                Some("auth verify".to_string()),
            );
            err_resp.data = Some(serde_json::json!({ "cleanup": cleanup }));
            output::print_json(&err_resp);
            EXIT_AUTH_REQUIRED
        }
        Ok(PendingLoginStatus::Expired) | Ok(PendingLoginStatus::Invalidated) => {
            let cleanup = finalize_dead_flow(store, &session);
            report_flow_error(
                "AUTH_FLOW_EXPIRED",
                "Authentication flow expired or was invalidated; restart with 'auth login'",
                false,
                EXIT_AUTH_REQUIRED,
                cleanup,
            )
        }
        Err(err) => {
            let cleanup = finalize_dead_flow(store, &session);
            let mapped = map_sso_error("Failed to resume login with captcha", &err)
                .unconfirmed_side_effect();
            report_flow_error(
                mapped.code,
                &mapped.message,
                mapped.retryable,
                mapped.exit_code,
                cleanup,
            )
        }
    }
}

async fn handle_logout_action(store: &StateStore) -> i32 {
    let client = match client_or_report() {
        Ok(client) => client,
        Err(code) => return code,
    };

    let _lock = match store::lock_or_report(store) {
        Ok(lock) => lock,
        Err(code) => return code,
    };

    // 服务端退出与本地清理分别记录，任何一方失败都不谎称全部完成
    let mut server_logout = false;
    let mut server_error = None;
    let mut read_error = None;

    match store.load_session() {
        Ok(mut session) => {
            let _ = cancel_pending_login(&mut session);
            match rsmycqu::sso::logout(&client, &mut session).await {
                // 退出登录是副作用动作：超时不代表未退出，禁止自动重试
                Ok(()) => server_logout = true,
                Err(err) => {
                    server_error = Some(
                        map_sso_error("Remote server logout failed", &err)
                            .unconfirmed_side_effect(),
                    );
                }
            }
        }
        // 没有会话、或待完成流程已过期：都没有可退出的服务端会话
        Err(store::StoreError::NotFound) | Err(store::StoreError::Expired(_)) => {}
        Err(err) => read_error = Some(err),
    }

    let local_result = store.clear_session();
    let local_cleared = local_result.is_ok();
    let local_error = local_result.err();

    // 读取失败不是“没有会话”：如实报告本地状态错误，并说明服务端退出未执行
    if let Some(err) = read_error {
        let (exit_code, err_code, message, retryable) = store::describe_store_error(&err);
        let mut resp =
            ApiResponse::<serde_json::Value>::err(err_code, &message, retryable, None, None);
        resp.data = Some(serde_json::json!({
            "server_logout": false,
            "local_cleared": local_cleared,
            "local_error": local_error,
        }));
        output::print_json(&resp);
        return exit_code;
    }

    if let Some(mapped) = server_error {
        // 服务端退出失败：退出码 5，同时如实报告本地清理结果
        let mut resp = ApiResponse::<serde_json::Value>::err(
            mapped.code,
            &mapped.message,
            mapped.retryable,
            None,
            None,
        );
        resp.data = Some(serde_json::json!({
            "server_logout": false,
            "local_cleared": local_cleared,
            "local_error": local_error,
        }));
        output::print_json(&resp);
        return mapped.exit_code;
    }

    if let Some(local_error) = local_error {
        let mut resp = ApiResponse::<serde_json::Value>::err(
            "STATE_INVALID",
            &format!("Remote logout succeeded but local cleanup failed: {local_error}"),
            false,
            None,
            None,
        );
        resp.data = Some(serde_json::json!({
            "server_logout": server_logout,
            "local_cleared": false,
        }));
        output::print_json(&resp);
        return output::EXIT_STATE_INVALID;
    }

    let message = if server_logout {
        "Logged out and local session cleared"
    } else {
        "No active session found; local state cleared"
    };
    output::print_json(&ApiResponse::success(
        serde_json::json!({
            "server_logout": server_logout,
            "local_cleared": true,
            "message": message,
        }),
        true,
        None,
    ));
    EXIT_SUCCESS
}

pub async fn handle_auth_command(cmd: AuthCommands, store: &StateStore) -> i32 {
    // 客户端在各分支内按需构造：auth status 只读本地状态时不需要网络栈
    match cmd {
        AuthCommands::Login(args) => handle_login_action(store, args).await,
        AuthCommands::Status(args) => handle_status_action(store, args).await,
        AuthCommands::Captcha => handle_captcha_action(store).await,
        AuthCommands::SendCode(args) => handle_send_code_action(store, args).await,
        AuthCommands::Verify(args) => handle_verify_action(store, args).await,
        AuthCommands::Logout => handle_logout_action(store).await,
    }
}
