use rsmycqu::mycqu::exam::Exam;
use rsmycqu::mycqu::User;

use crate::client::{client_or_report, report_api_error};
use crate::commands::access::{Service, authorized_session};
use crate::output::{self, ApiResponse, EXIT_SUCCESS};
use crate::store::StateStore;

#[derive(clap::Subcommand)]
pub enum ExamCommands {
    #[command(about = "查询考试安排列表")]
    List,
}

pub async fn handle_exam(cmd: ExamCommands, store: &StateStore) -> i32 {
    let ExamCommands::List = cmd;
    let client = match client_or_report() {
        Ok(client) => client,
        Err(code) => return code,
    };

    let session = match authorized_session(store, &client, Service::Mycqu).await {
        Ok(session) => session,
        Err(code) => return code,
    };

    let user = match User::fetch_self(&client, &session).await {
        Ok(user) => user,
        Err(e) => return report_api_error("Failed to fetch current user profile", &e),
    };

    match Exam::fetch_all(&client, &session, &user.id).await {
        Ok(exams) => {
            let count = exams.len();
            output::print_json(&ApiResponse::success(exams, true, Some(count)));
            EXIT_SUCCESS
        }
        Err(e) => report_api_error("Failed to fetch exams", &e),
    }
}
