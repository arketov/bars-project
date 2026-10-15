//! `bars-cli`: CLI для людей, хуков и агентов (Т-20). Тонкий клиент воркера: ядра в нём нет.

use std::io::{Read, Write};
use std::process::ExitCode;

use bars_cli::{Cli, Env};
use clap::Parser;

fn main() -> ExitCode {
    let cli = Cli::parse();
    let env = match Env::current() {
        Ok(mut env) => {
            if matches!(cli.command, bars_cli::Command::Hook { .. }) {
                let mut input = String::new();
                if std::io::stdin().read_to_string(&mut input).is_ok() {
                    env.hook_input = Some(input);
                }
            }
            env
        }
        Err(error) => {
            eprintln!("bars-cli: {error}");
            return ExitCode::from(bars_protocol::EXIT_FAILURE as u8);
        }
    };
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("bars-cli: не удалось запустить рантайм: {error}");
            return ExitCode::from(bars_protocol::EXIT_FAILURE as u8);
        }
    };
    let mut out = std::io::stdout().lock();
    let mut err = std::io::stderr().lock();
    let code = runtime.block_on(bars_cli::run(cli, &env, &mut out, &mut err));
    let _ = out.flush();
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}
