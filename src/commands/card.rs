use chrono::NaiveDate;
use clap::{Args, Subcommand};
use rsmycqu::card::Card;

use crate::client::{client_or_report, report_api_error};
use crate::commands::access::{Service, authorized_session};
use crate::output::{self, ApiResponse, EXIT_INCOMPLETE, EXIT_SUCCESS};
use crate::store::StateStore;

#[derive(Subcommand)]
pub enum CardCommands {
    #[command(about = "查询校园卡余额")]
    Balance,
    #[command(about = "查询校园卡消费账单")]
    Bills(BillsArgs),
}

#[derive(Args)]
pub struct BillsArgs {
    #[arg(short, long, help = "开始日期 (YYYY-MM-DD)")]
    pub start_date: String,
    #[arg(short, long, help = "结束日期 (YYYY-MM-DD)")]
    pub end_date: String,
    #[arg(long, default_value = "1", help = "分页页码 (从 1 开始)")]
    pub page: u16,
    #[arg(long, default_value = "100", help = "每页行数 (大于 0)")]
    pub row: u16,
}

/// 校验账单查询参数；在发起会话加载与网络请求前拦截。
fn validate_bills_args(args: &BillsArgs) -> Result<(), String> {
    if args.page == 0 {
        return Err("Page number cannot be 0, must be >= 1".to_string());
    }
    if args.row == 0 {
        return Err("Row count cannot be 0, must be >= 1".to_string());
    }

    let start_date = NaiveDate::parse_from_str(&args.start_date, "%Y-%m-%d")
        .map_err(|_| "Invalid start_date format, expected YYYY-MM-DD".to_string())?;
    let end_date = NaiveDate::parse_from_str(&args.end_date, "%Y-%m-%d")
        .map_err(|_| "Invalid end_date format, expected YYYY-MM-DD".to_string())?;

    if start_date > end_date {
        return Err(format!(
            "start_date ({}) cannot be later than end_date ({})",
            args.start_date, args.end_date
        ));
    }
    Ok(())
}

pub async fn handle_card(cmd: CardCommands, store: &StateStore) -> i32 {
    let bills_args = match cmd {
        CardCommands::Balance => None,
        CardCommands::Bills(args) => Some(args),
    };

    if let Some(args) = &bills_args
        && let Err(msg) = validate_bills_args(args)
    {
        output::print_err("INVALID_ARGUMENT", &msg, false, None, None);
        return output::EXIT_INVALID_ARGUMENT;
    }

    let client = match client_or_report() {
        Ok(client) => client,
        Err(code) => return code,
    };

    let session = match authorized_session(store, &client, Service::Card).await {
        Ok(session) => session,
        Err(code) => return code,
    };

    let card = match Card::fetch_self(&client, &session).await {
        Ok(card) => card,
        Err(e) => return report_api_error("Failed to fetch card account info", &e),
    };

    let Some(args) = bills_args else {
        output::print_json(&ApiResponse::success(card, true, None));
        return EXIT_SUCCESS;
    };

    match card.fetch_bill(&client, &session, &args.start_date, &args.end_date, args.page, args.row).await {
        Ok(bills) => {
            let count = bills.len();
            let is_full_page = count >= usize::from(args.row);
            let mut resp = ApiResponse::success(bills, !is_full_page, Some(count));
            if is_full_page {
                resp.meta.stop_reason = Some(format!("Page {} reached full row capacity ({} items); additional bills may exist on subsequent pages", args.page, args.row));
                output::print_json(&resp);
                EXIT_INCOMPLETE
            } else {
                output::print_json(&resp);
                EXIT_SUCCESS
            }
        }
        Err(e) => report_api_error("Failed to fetch bills", &e),
    }
}
