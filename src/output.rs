use serde::Serialize;

pub const EXIT_SUCCESS: i32 = 0;
pub const EXIT_INVALID_ARGUMENT: i32 = 2;
pub const EXIT_AUTH_REQUIRED: i32 = 3;
pub const EXIT_PERMISSION_DENIED: i32 = 4;
pub const EXIT_UPSTREAM_ERROR: i32 = 5;
pub const EXIT_STATE_INVALID: i32 = 6;
pub const EXIT_INCOMPLETE: i32 = 7;

#[derive(Serialize)]
pub struct ApiResponse<T: Serialize> {
    pub schema_version: u32,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ApiErrorPayload>,
    pub meta: ApiMeta,
}

#[derive(Serialize)]
pub struct ApiErrorPayload {
    pub code: String,
    pub message: String,
    pub retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub challenge_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_action: Option<String>,
}

/// 已解析的查询范围：Agent 据此解释结果，不必猜测服务端如何理解参数。
#[derive(Serialize)]
pub struct QueryRange {
    /// 实际解析出的学期 ID
    pub term_id: u16,
    /// 实际过滤的日期（`YYYY-MM-DD`）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    /// 实际过滤的教学周次
    #[serde(skip_serializing_if = "Option::is_none")]
    pub week: Option<u8>,
}

#[derive(Serialize)]
pub struct ApiMeta {
    pub complete: bool,
    pub timezone: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checked_service: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checked_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query_range: Option<QueryRange>,
}

impl<T: Serialize> ApiResponse<T> {
    pub fn success(data: T, complete: bool, count: Option<usize>) -> Self {
        Self {
            schema_version: 1,
            ok: true,
            data: Some(data),
            error: None,
            meta: ApiMeta {
                complete,
                timezone: "Asia/Shanghai".to_string(),
                count,
                stop_reason: None,
                checked_service: None,
                checked_at: None,
                query_range: None,
            },
        }
    }

    pub fn err(code: &str, message: &str, retryable: bool, challenge_id: Option<String>, next_action: Option<String>) -> ApiResponse<serde_json::Value> {
        ApiResponse {
            schema_version: 1,
            ok: false,
            data: None,
            error: Some(ApiErrorPayload {
                code: code.to_string(),
                message: message.to_string(),
                retryable,
                challenge_id,
                next_action,
            }),
            meta: ApiMeta {
                complete: false,
                timezone: "Asia/Shanghai".to_string(),
                count: None,
                stop_reason: None,
                checked_service: None,
                checked_at: None,
                query_range: None,
            },
        }
    }
}

pub fn print_json<T: Serialize>(resp: &ApiResponse<T>) {
    match serde_json::to_string_pretty(resp) {
        Ok(json) => println!("{json}"),
        Err(e) => {
            // 绝不静默丢弃输出：序列化失败按上游错误报告，避免“退出码 0 但无输出”
            let fallback = ApiResponse::<serde_json::Value>::err(
                "UPSTREAM_ERROR",
                &format!("Failed to serialize response: {e}"),
                false,
                None,
                None,
            );
            match serde_json::to_string_pretty(&fallback) {
                Ok(json) => {
                    println!("{json}");
                    std::process::exit(EXIT_UPSTREAM_ERROR);
                }
                Err(_) => std::process::exit(EXIT_UPSTREAM_ERROR),
            }
        }
    }
}

pub fn print_err(code: &str, message: &str, retryable: bool, challenge_id: Option<String>, next_action: Option<String>) {
    let resp = ApiResponse::<serde_json::Value>::err(code, message, retryable, challenge_id, next_action);
    print_json(&resp);
}
