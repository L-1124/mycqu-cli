use clap::{Args, Subcommand};
use rsmycqu::mycqu::course::{CQUSession, CQUSessionInfo};

use crate::client::{client_or_report, report_api_error};
use crate::commands::access::{Service, authorized_session};
use crate::output::{self, ApiResponse, EXIT_SUCCESS};
use crate::store::StateStore;

#[derive(Subcommand)]
pub enum TermCommands {
    #[command(about = "列出所有学期")]
    List,
    #[command(about = "显示当前学期详细信息")]
    Current,
    #[command(about = "获取指定学期详细信息")]
    Show(TermShowArgs),
}

#[derive(Args)]
pub struct TermShowArgs {
    #[arg(short, long, help = "学期 ID")]
    pub id: u32,
}

pub async fn handle_term(cmd: TermCommands, store: &StateStore) -> i32 {
    let client = match client_or_report() {
        Ok(client) => client,
        Err(code) => return code,
    };

    let session = match authorized_session(store, &client, Service::Mycqu).await {
        Ok(session) => session,
        Err(code) => return code,
    };

    match cmd {
        TermCommands::List => match CQUSession::fetch_all(&client, &session).await {
            Ok(list) => {
                let count = list.len();
                output::print_json(&ApiResponse::success(list, true, Some(count)));
                EXIT_SUCCESS
            }
            Err(e) => report_api_error("Failed to fetch session list", &e),
        },
        TermCommands::Current => match CQUSessionInfo::fetch_curr(&client, &session).await {
            Ok(curr) => {
                output::print_json(&ApiResponse::success(curr, true, None));
                EXIT_SUCCESS
            }
            Err(e) => report_api_error("Failed to fetch current session info", &e),
        },
        TermCommands::Show(args) => match CQUSessionInfo::fetch_detail(&client, &session, args.id).await {
            Ok(detail) => {
                output::print_json(&ApiResponse::success(detail, true, None));
                EXIT_SUCCESS
            }
            Err(e) => report_api_error("Failed to fetch session detail", &e),
        },
    }
}
