use clap::{Parser, Subcommand};
use std::fmt::Display;

#[derive(Parser, Debug)]
#[command(version, about)]
#[command(next_line_help = true)]
pub struct Cli {
    #[arg(default_value = "sample.db")]
    pub db_path: String,
    #[command(subcommand)]
    pub cmd: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    #[command(name = ".dbinfo")]
    DbInfo,
}

impl Display for Command {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Command::DbInfo => write!(f, "DbInfo"),
        }
    }
}
