use clap::{Args, Subcommand};
use rsmycqu::mycqu::course::{CQUSession, CQUSessionInfo};
use serde::Serialize;

use crate::client::{client_or_report, report_api_error};
use crate::commands::access::{Service, authorized_session};
use crate::output::{self, ApiResponse, EXIT_SUCCESS};
use crate::store::StateStore;

#[derive(Serialize)]
pub struct TermItem {
    pub id: Option<u16>,
    pub name: String,
    pub year: u16,
    pub is_autumn: bool,
}

impl From<CQUSession> for TermItem {
    fn from(s: CQUSession) -> Self {
        let name = s.to_string();
        Self {
            id: s.id,
            name,
            year: s.year,
            is_autumn: s.is_autumn,
        }
    }
}

#[derive(Serialize)]
pub struct TermInfoItem {
    pub id: Option<u16>,
    pub name: String,
    pub year: u16,
    pub is_autumn: bool,
    pub begin_date_str: Option<String>,
    pub end_date_str: Option<String>,
    pub active: bool,
}

impl From<CQUSessionInfo> for TermInfoItem {
    fn from(info: CQUSessionInfo) -> Self {
        let name = info.session.to_string();
        Self {
            id: info.session.id,
            name,
            year: info.session.year,
            is_autumn: info.session.is_autumn,
            begin_date_str: info.begin_date_str,
            end_date_str: info.end_date_str,
            active: info.active,
        }
    }
}
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
                let items: Vec<TermItem> = list.into_iter().map(TermItem::from).collect();
                let count = items.len();
                output::print_json(&ApiResponse::success(items, true, Some(count)));
                EXIT_SUCCESS
            }
            Err(e) => report_api_error("Failed to fetch session list", &e),
        },
        TermCommands::Current => match CQUSessionInfo::fetch_curr(&client, &session).await {
            Ok(curr) => {
                let item = TermInfoItem::from(curr);
                output::print_json(&ApiResponse::success(item, true, None));
                EXIT_SUCCESS
            }
            Err(e) => report_api_error("Failed to fetch current session info", &e),
        },
        TermCommands::Show(args) => {
            match CQUSessionInfo::fetch_detail(&client, &session, args.id).await {
                Ok(detail) => {
                    let item = TermInfoItem::from(detail);
                    output::print_json(&ApiResponse::success(item, true, None));
                    EXIT_SUCCESS
                }
                Err(e) => report_api_error("Failed to fetch session detail", &e),
            }
        }
    }
}
