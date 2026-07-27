use ghostpost_evals::run_cli;
use std::process::ExitCode as StdExit;

#[tokio::main]
async fn main() -> StdExit {
    match run_cli().await {
        Ok(code) => code.into(),
        Err(err) => {
            eprintln!("error: {err:#}");
            ghostpost_evals::ExitCode::UserError.into()
        }
    }
}
