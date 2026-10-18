use rsmycqu::mycqu::User;

use crate::client::{client_or_report, report_api_error};
use crate::commands::access::{Service, authorized_session};
use crate::output::{self, ApiResponse, EXIT_SUCCESS};
use crate::store::StateStore;

#[derive(clap::Subcommand)]
pub enum UserCommands {
    #[command(about = "查看当前登录用户信息")]
    Show,
}

pub async fn handle_user(cmd: UserCommands, store: &StateStore) -> i32 {
    let UserCommands::Show = cmd;
    let client = match client_or_report() {
        Ok(client) => client,
        Err(code) => return code,
    };

    let session = match authorized_session(store, &client, Service::Mycqu).await {
        Ok(session) => session,
        Err(code) => return code,
    };

    match User::fetch_self(&client, &session).await {
        Ok(user) => {
            output::print_json(&ApiResponse::success(user, true, None));
            EXIT_SUCCESS
        }
        Err(e) => report_api_error("Failed to fetch user profile", &e),
    }
}
