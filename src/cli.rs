use clap::{Parser, Subcommand};

use crate::auth::AuthCommands;
use crate::commands::{
    CardCommands, CourseCommands, DormCommands, ExamCommands, ScheduleCommands, ScoreCommands,
    SkillCommands, TermCommands, UserCommands,
};

#[derive(Parser)]
#[command(
    name = "mycqu",
    author = "CQU Agent Working Group",
    version = "0.1.0",
    about = "CLI for Chongqing University campus services, structured for Agent automation."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    #[command(about = "统一身份认证与会话管理")]
    Auth {
        #[command(subcommand)]
        cmd: AuthCommands,
    },
    #[command(about = "个人信息与档案")]
    User {
        #[command(subcommand)]
        cmd: UserCommands,
    },
    #[command(about = "学期信息与校历")]
    Term {
        #[command(subcommand)]
        cmd: TermCommands,
    },
    #[command(about = "课程表与上课安排")]
    Schedule {
        #[command(subcommand)]
        cmd: ScheduleCommands,
    },
    #[command(about = "成绩与绩点排名")]
    Score {
        #[command(subcommand)]
        cmd: ScoreCommands,
    },
    #[command(about = "可选课程目录与教学班详情")]
    Course {
        #[command(subcommand)]
        cmd: CourseCommands,
    },
    #[command(about = "考试安排日程")]
    Exam {
        #[command(subcommand)]
        cmd: ExamCommands,
    },
    #[command(about = "校园卡余额与消费流水")]
    Card {
        #[command(subcommand)]
        cmd: CardCommands,
    },
    #[command(about = "宿舍水电费查询")]
    Dorm {
        #[command(subcommand)]
        cmd: DormCommands,
    },
    #[command(about = "管理与分发 Agent Skills 到各 AI 工具环境")]
    Skill {
        #[command(subcommand)]
        cmd: SkillCommands,
    },
    #[command(about = "查询 CLI 的接口 Schema 与参数定义")]
    Schema {
        #[arg(help = "可选的子命令路径，如 schedule list")]
        path: Vec<String>,
    },
}
