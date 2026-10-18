use chrono::{Datelike, NaiveDate};
use clap::{Args, Subcommand};
use rsmycqu::mycqu::course::{CQUSessionInfo, CourseTimetable};
use rsmycqu::session::{Client, Session};

use crate::client::{client_or_report, report_api_error};
use crate::commands::access::{Service, authorized_session};
use crate::output::{self, ApiResponse, QueryRange, EXIT_SUCCESS};
use crate::store::StateStore;

#[derive(Subcommand)]
pub enum ScheduleCommands {
    #[command(about = "查询课程表")]
    List(ScheduleListArgs),
}

#[derive(Args)]
pub struct ScheduleListArgs {
    #[arg(short, long, help = "学期标识：数字 ID 或 'current'")]
    pub term: Option<String>,
    #[arg(long, help = "指定日期 (YYYY-MM-DD) 进行精准过滤")]
    pub date: Option<String>,
    #[arg(short, long, help = "指定周次过滤")]
    pub week: Option<u8>,
}

/// 提前校验 `--term`：数字 ID 或 `current`，避免无效调用完成网络请求后才被拒绝。
fn validate_term(term: Option<&str>) -> Result<(), String> {
    match term {
        Some(value) if !value.eq_ignore_ascii_case("current") && value.parse::<u16>().is_err() => {
            Err("Invalid --term argument; expected integer ID or 'current'".to_string())
        }
        _ => Ok(()),
    }
}

async fn resolve_session_id(
    client: &Client,
    session: &Session,
    term: Option<&str>,
) -> Result<u16, i32> {
    let needs_upstream = match term {
        Some(value) => value.eq_ignore_ascii_case("current"),
        None => true,
    };

    if !needs_upstream {
        // 已在 validate_term 中校验过，这里只做类型转换
        return term
            .and_then(|value| value.parse::<u16>().ok())
            .ok_or(output::EXIT_INVALID_ARGUMENT);
    }

    let curr = match CQUSessionInfo::fetch_curr(client, session).await {
        Ok(curr) => curr,
        Err(e) => return Err(report_api_error("Failed to get current session from upstream", &e)),
    };
    match curr.session.id {
        Some(id) => Ok(id),
        None => {
            output::print_err(
                "STATE_INVALID",
                "Current session has no valid ID",
                false,
                None,
                None,
            );
            Err(output::EXIT_STATE_INVALID)
        }
    }
}

async fn filter_by_date(
    client: &Client,
    session: &Session,
    session_id: u16,
    target_date: NaiveDate,
    timetables: Vec<CourseTimetable>,
) -> i32 {
    let curr_session_info = match CQUSessionInfo::fetch_detail(client, session, u32::from(session_id)).await {
        Ok(info) => info,
        Err(e) => return report_api_error("Failed to fetch session detail for date mapping", &e),
    };

    let Some(begin_str) = curr_session_info.begin_date_str else {
        output::print_err("UPSTREAM_ERROR", "Session info missing begin_date_str", false, None, None);
        return output::EXIT_UPSTREAM_ERROR;
    };
    let Ok(begin_date) = NaiveDate::parse_from_str(begin_str.split_whitespace().next().unwrap_or(""), "%Y-%m-%d") else {
        output::print_err("UPSTREAM_ERROR", "Failed to parse session begin date", false, None, None);
        return output::EXIT_UPSTREAM_ERROR;
    };

    let date_str = target_date.to_string();
    let days_diff = target_date.signed_duration_since(begin_date).num_days();
    if days_diff < 0 {
        let mut resp = ApiResponse::success(Vec::<CourseTimetable>::new(), true, Some(0));
        resp.meta.query_range = Some(QueryRange {
            term_id: session_id,
            date: Some(date_str),
            week: None,
        });
        output::print_json(&resp);
        return EXIT_SUCCESS;
    }

    let Ok(days_diff_u8) = u8::try_from(days_diff / 7) else {
        let mut resp = ApiResponse::success(Vec::<CourseTimetable>::new(), true, Some(0));
        resp.meta.query_range = Some(QueryRange {
            term_id: session_id,
            date: Some(date_str),
            week: None,
        });
        output::print_json(&resp);
        return EXIT_SUCCESS;
    };
    let week_num = days_diff_u8 + 1;
    let Ok(target_weekday) = u8::try_from(target_date.weekday().num_days_from_monday()) else {
        output::print_err("STATE_INVALID", "Invalid weekday calculation", false, None, None);
        return output::EXIT_STATE_INVALID;
    };

    let filtered: Vec<CourseTimetable> = timetables
        .into_iter()
        .filter(|item| {
            let matches_week = item.weeks.iter().any(|p| week_num >= p.start && week_num <= p.end);
            let matches_weekday = item
                .day_time
                .as_ref()
                .is_some_and(|dt| dt.weekday == target_weekday);
            matches_week && matches_weekday
        })
        .collect();

    let count = filtered.len();
    let mut resp = ApiResponse::success(filtered, true, Some(count));
    resp.meta.query_range = Some(QueryRange {
        term_id: session_id,
        date: Some(date_str),
        week: Some(week_num),
    });
    output::print_json(&resp);
    EXIT_SUCCESS
}

pub async fn handle_schedule(cmd: ScheduleCommands, store: &StateStore) -> i32 {
    let ScheduleCommands::List(args) = cmd;

    // 前置严格参数校验：在发起任何网络请求前执行
    if args.date.is_some() && args.week.is_some() {
        output::print_err(
            "INVALID_ARGUMENT",
            "Cannot specify both --date and --week simultaneously; choose one filtering strategy",
            false,
            None,
            None,
        );
        return output::EXIT_INVALID_ARGUMENT;
    }

    if args.week == Some(0) {
        output::print_err("INVALID_ARGUMENT", "Invalid week number: week cannot be 0", false, None, None);
        return output::EXIT_INVALID_ARGUMENT;
    }

    if let Err(msg) = validate_term(args.term.as_deref()) {
        output::print_err("INVALID_ARGUMENT", &msg, false, None, None);
        return output::EXIT_INVALID_ARGUMENT;
    }

    let parsed_date = match args.date.as_deref() {
        Some(value) => match NaiveDate::parse_from_str(value, "%Y-%m-%d") {
            Ok(date) => Some(date),
            Err(_) => {
                output::print_err("INVALID_ARGUMENT", "Invalid date format, expected YYYY-MM-DD", false, None, None);
                return output::EXIT_INVALID_ARGUMENT;
            }
        },
        None => None,
    };

    let client = match client_or_report() {
        Ok(client) => client,
        Err(code) => return code,
    };

    let session = match authorized_session(store, &client, Service::Mycqu).await {
        Ok(session) => session,
        Err(code) => return code,
    };

    let user = match rsmycqu::mycqu::User::fetch_self(&client, &session).await {
        Ok(user) => user,
        Err(e) => return report_api_error("Failed to fetch current user profile", &e),
    };

    let session_id = match resolve_session_id(&client, &session, args.term.as_deref()).await {
        Ok(id) => id,
        Err(code) => return code,
    };

    let timetables = match CourseTimetable::fetch_curr(&client, &session, &user.id, session_id).await {
        Ok(timetables) => timetables,
        Err(e) => return report_api_error("Failed to fetch timetable", &e),
    };

    if let Some(target_date) = parsed_date {
        return filter_by_date(&client, &session, session_id, target_date, timetables).await;
    }

    if let Some(target_week) = args.week {
        let filtered: Vec<CourseTimetable> = timetables
            .into_iter()
            .filter(|item| item.weeks.iter().any(|p| target_week >= p.start && target_week <= p.end))
            .collect();
        let count = filtered.len();
        let mut resp = ApiResponse::success(filtered, true, Some(count));
        resp.meta.query_range = Some(QueryRange {
            term_id: session_id,
            date: None,
            week: Some(target_week),
        });
        output::print_json(&resp);
        return EXIT_SUCCESS;
    }

    let count = timetables.len();
    let mut resp = ApiResponse::success(timetables, true, Some(count));
    resp.meta.query_range = Some(QueryRange {
        term_id: session_id,
        date: None,
        week: None,
    });
    output::print_json(&resp);
    EXIT_SUCCESS
}
