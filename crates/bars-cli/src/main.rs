//! `bars-cli`: CLI для людей, хуков и агентов (Т-20). Срез 0: каркас.

use clap::Parser;

#[derive(Parser)]
#[command(name = "bars-cli", version, about = "CLI для bars")]
struct Cli {}

fn main() {
    Cli::parse();
}
