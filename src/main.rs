use clap::{Parser, error::ErrorKind};

use mycqu_cli::auth::handle_auth_command;
use mycqu_cli::cli::{Cli, Commands};
use mycqu_cli::commands::{
    handle_card, handle_course, handle_dorm, handle_exam, handle_schedule, handle_score,
    handle_skill, handle_term, handle_user,
};
use mycqu_cli::output::{self, ApiResponse};
use mycqu_cli::schema::schema_catalog;
use mycqu_cli::store::StateStore;

#[tokio::main]
async fn main() {
    std::process::exit(run().await);
}

async fn run() -> i32 {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) => {
            // 帮助与版本请求走传统输出，与真正的参数错误区分
            if matches!(
                err.kind(),
                ErrorKind::DisplayHelp
                    | ErrorKind::DisplayVersion
                    | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
            ) {
                let _ = err.print();
                return output::EXIT_SUCCESS;
            }
            output::print_err(
                "INVALID_ARGUMENT",
                &format!("Command-line argument parsing failed: {err}"),
                false,
                None,
                Some("mycqu schema".to_string()),
            );
            return output::EXIT_INVALID_ARGUMENT;
        }
    };

    // schema 是能力发现入口，不依赖本地状态存储（含平台限制）
    if let Commands::Schema { ref path } = cli.command {
        return handle_schema(path);
    }
    if let Commands::Skill { cmd } = cli.command {
        return handle_skill(cmd);
    }

    let store = match StateStore::new() {
        Ok(store) => store,
        Err(e) => {
            output::print_err(
                "STATE_INVALID",
                &format!("Failed to initialize local state store: {e}"),
                false,
                None,
                None,
            );
            return output::EXIT_STATE_INVALID;
        }
    };

    match cli.command {
        Commands::Auth { cmd } => handle_auth_command(cmd, &store).await,
        Commands::User { cmd } => handle_user(cmd, &store).await,
        Commands::Term { cmd } => handle_term(cmd, &store).await,
        Commands::Schedule { cmd } => handle_schedule(cmd, &store).await,
        Commands::Score { cmd } => handle_score(cmd, &store).await,
        Commands::Course { cmd } => handle_course(cmd, &store).await,
        Commands::Exam { cmd } => handle_exam(cmd, &store).await,
        Commands::Card { cmd } => handle_card(cmd, &store).await,
        Commands::Dorm { cmd } => handle_dorm(cmd, &store).await,
        Commands::Skill { cmd } => handle_skill(cmd),
        Commands::Schema { path } => handle_schema(&path),
    }
}

fn handle_schema(path: &[String]) -> i32 {
    // 目录解析失败必须显式报错，不允许静默回退为空命令目录
    let catalog = match schema_catalog() {
        Ok(catalog) => catalog,
        Err(e) => {
            output::print_err(
                "STATE_INVALID",
                &format!("Schema catalog unavailable: {e}"),
                false,
                None,
                None,
            );
            return output::EXIT_STATE_INVALID;
        }
    };

    if path.is_empty() {
        output::print_json(&ApiResponse::success(catalog, true, None));
    } else {
        let query = path.join(" ");
        let Some(cmds) = catalog["commands"].as_array() else {
            output::print_err(
                "STATE_INVALID",
                "Malformed schema catalog",
                false,
                None,
                None,
            );
            return output::EXIT_STATE_INVALID;
        };
        if let Some(cmd) = cmds.iter().find(|c| c["path"].as_str() == Some(&query)) {
            output::print_json(&ApiResponse::success(cmd, true, None));
        } else {
            let prefix = format!("{query} ");
            let group_cmds: Vec<&serde_json::Value> = cmds
                .iter()
                .filter(|c| c["path"].as_str().is_some_and(|p| p.starts_with(&prefix)))
                .collect();

            if !group_cmds.is_empty() {
                let count = group_cmds.len();
                let group_data = serde_json::json!({
                    "group": query,
                    "commands": group_cmds,
                });
                output::print_json(&ApiResponse::success(group_data, true, Some(count)));
            } else {
                output::print_err(
                    "INVALID_ARGUMENT",
                    &format!("Command or group '{query}' not found in schema"),
                    false,
                    None,
                    None,
                );
                return output::EXIT_INVALID_ARGUMENT;
            }
        }
    }
    output::EXIT_SUCCESS
}
