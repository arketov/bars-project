//! `bars-mcp`: stdio-MCP для агентских клиентов (Т-2). Тонкий прокси воркера: ядра в нём нет.

use std::process::ExitCode;

fn main() -> ExitCode {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("bars-mcp: не удалось запустить рантайм: {error}");
            return ExitCode::FAILURE;
        }
    };
    let result = bars_mcp::Env::current().map(|env| runtime.block_on(bars_mcp::run_stdio(env)));
    match result {
        Ok(Ok(())) => ExitCode::SUCCESS,
        Ok(Err(error)) | Err(error) => {
            eprintln!("bars-mcp: {error}");
            ExitCode::FAILURE
        }
    }
}
