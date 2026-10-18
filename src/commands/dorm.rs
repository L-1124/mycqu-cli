use clap::{Args, Subcommand};
use rsmycqu::card::{EnergyFees, access_card};

use crate::client::{client_or_report, map_api_error, report_api_error};
use crate::output::{self, ApiResponse, EXIT_SUCCESS};
use crate::store::{self, StateStore};

#[derive(Subcommand)]
pub enum DormCommands {
    #[command(about = "查询宿舍水电费余额")]
    Balance(DormBalanceArgs),
}

#[derive(Args)]
pub struct DormBalanceArgs {
    #[arg(short, long, help = "房间号或房间标识")]
    pub room: String,
    #[arg(long, help = "是否为虎溪校区")]
    pub is_huxi: bool,
}

pub async fn handle_dorm(cmd: DormCommands, store: &StateStore) -> i32 {
    let DormCommands::Balance(args) = cmd;
    let client = match client_or_report() {
        Ok(client) => client,
        Err(code) => return code,
    };

    // 与其它命令不同，本次查询会修改会话（缓存宿舍服务凭据），
    // 因此临界区必须延伸到保存为止，不能复用 commands::access::authorized_session。
    let fee = {
        let _lock = match store::lock_or_report(store) {
            Ok(lock) => lock,
            Err(code) => return code,
        };
        let mut session = match store::load_session_or_report(store) {
            Ok(session) => session,
            Err(code) => return code,
        };

        if let Err(e) = access_card(&client, &mut session).await {
            let mapped = map_api_error("Failed to access card service", &e);
            mapped.report();
            return mapped.exit_code;
        }

        match EnergyFees::fetch_self(&client, &mut session, &args.room, args.is_huxi).await {
            Ok(fee) => {
                if let Err(code) = store::save_session_or_report(store, &session) {
                    return code;
                }
                fee
            }
            Err(e) => return report_api_error("Failed to fetch energy fees", &e),
        }
    };

    output::print_json(&ApiResponse::success(fee, true, None));
    EXIT_SUCCESS
}
