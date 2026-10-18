use clap::{Args, Subcommand};
use rsmycqu::mycqu::course::CQUSessionInfo;
use rsmycqu::mycqu::score::{GPARanking, Score};
use rsmycqu::session::{Client, Session};

use crate::client::{client_or_report, report_api_error};
use crate::commands::access::{Service, authorized_session};
use crate::output::{self, ApiResponse, EXIT_SUCCESS};
use crate::store::StateStore;

#[derive(Subcommand)]
pub enum ScoreCommands {
    #[command(about = "获取成绩列表")]
    List(ScoreListArgs),
    #[command(about = "获取绩点与排名")]
    Gpa,
}

#[derive(Args)]
pub struct ScoreListArgs {
    #[arg(long, help = "是否为辅修")]
    pub is_minor: bool,
    #[arg(long, help = "按学期过滤：支持数字年份（如 2024）或 'current'")]
    pub term: Option<String>,
}

/// 提前校验 `--term`，避免无效调用在完成网络请求后才被拒绝。
fn validate_term(term: Option<&str>) -> Result<(), String> {
    match term {
        Some(value) if !value.eq_ignore_ascii_case("current") && value.parse::<u16>().is_err() => {
            Err("Invalid --term argument; expected year number or 'current'".to_string())
        }
        _ => Ok(()),
    }
}

async fn resolve_filter_year(
    client: &Client,
    session: &Session,
    term: Option<&str>,
) -> Result<Option<u16>, i32> {
    let Some(term) = term else {
        return Ok(None);
    };
    if !term.eq_ignore_ascii_case("current") {
        // 已在 validate_term 中校验过，这里只做类型转换
        return term
            .parse::<u16>()
            .map(Some)
            .map_err(|_| output::EXIT_INVALID_ARGUMENT);
    }

    match CQUSessionInfo::fetch_curr(client, session).await {
        Ok(curr) => Ok(Some(curr.session.year)),
        Err(e) => Err(report_api_error("Failed to resolve current session for score filter", &e)),
    }
}

pub async fn handle_score(cmd: ScoreCommands, store: &StateStore) -> i32 {
    // 先匹配子命令并做纯参数校验，再决定是否需要会话与网络
    let list_args = match cmd {
        ScoreCommands::List(args) => Some(args),
        ScoreCommands::Gpa => None,
    };
    if let Some(args) = &list_args
        && let Err(msg) = validate_term(args.term.as_deref())
    {
        output::print_err("INVALID_ARGUMENT", &msg, false, None, None);
        return output::EXIT_INVALID_ARGUMENT;
    }

    let client = match client_or_report() {
        Ok(client) => client,
        Err(code) => return code,
    };

    let session = match authorized_session(store, &client, Service::Mycqu).await {
        Ok(session) => session,
        Err(code) => return code,
    };

    if let Some(args) = list_args {
        let filter_year = match resolve_filter_year(&client, &session, args.term.as_deref()).await {
            Ok(year) => year,
            Err(code) => return code,
        };

        match Score::fetch_self(&client, &session, args.is_minor).await {
            Ok(scores) => {
                let filtered: Vec<Score> = match filter_year {
                    Some(year) => scores.into_iter().filter(|s| s.session.year == year).collect(),
                    None => scores,
                };
                let count = filtered.len();
                output::print_json(&ApiResponse::success(filtered, true, Some(count)));
                EXIT_SUCCESS
            }
            Err(e) => report_api_error("Failed to fetch scores", &e),
        }
    } else {
        match GPARanking::fetch_self(&client, &session).await {
            Ok(gpa) => {
                output::print_json(&ApiResponse::success(gpa, true, None));
                EXIT_SUCCESS
            }
            Err(e) => report_api_error("Failed to fetch GPA ranking", &e),
        }
    }
}
