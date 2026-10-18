use clap::{Args, Subcommand};
use rsmycqu::mycqu::enroll::{EnrollCourseInfo, EnrollCourseItem};

use crate::client::{client_or_report, report_api_error};
use crate::commands::access::{Service, authorized_session};
use crate::output::{self, ApiResponse, EXIT_SUCCESS};
use crate::store::StateStore;

#[derive(Subcommand)]
pub enum CourseCommands {
    #[command(about = "查询可选课程列表目录")]
    List(CourseListArgs),
    #[command(about = "查询指定课程的具体教学班级详情")]
    Classes(CourseClassesArgs),
}

#[derive(Args)]
pub struct CourseListArgs {
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set, help = "是否为主修课程 (true/false)")]
    pub is_major: bool,
}

#[derive(Args)]
pub struct CourseClassesArgs {
    #[arg(short, long, help = "课程 ID（如从 course list 中获取）")]
    pub course_id: String,
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set, help = "是否为主修课程 (true/false)")]
    pub is_major: bool,
}

pub async fn handle_course(cmd: CourseCommands, store: &StateStore) -> i32 {
    let client = match client_or_report() {
        Ok(client) => client,
        Err(code) => return code,
    };

    let session = match authorized_session(store, &client, Service::Mycqu).await {
        Ok(session) => session,
        Err(code) => return code,
    };

    match cmd {
        CourseCommands::List(args) => match EnrollCourseInfo::fetch_all(&client, &session, args.is_major).await {
            Ok(courses_map) => {
                let total = courses_map.values().map(Vec::len).sum();
                output::print_json(&ApiResponse::success(courses_map, true, Some(total)));
                EXIT_SUCCESS
            }
            Err(e) => report_api_error("Failed to fetch course list", &e),
        },
        CourseCommands::Classes(args) => {
            match EnrollCourseItem::fetch_all(&client, &session, &args.course_id, args.is_major).await {
                Ok(items) => {
                    let count = items.len();
                    output::print_json(&ApiResponse::success(items, true, Some(count)));
                    EXIT_SUCCESS
                }
                Err(e) => report_api_error("Failed to fetch class items", &e),
            }
        }
    }
}
