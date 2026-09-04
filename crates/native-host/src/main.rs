use std::io::{stdin, stdout};
use std::process::ExitCode;

use download_manager_native_host::{HostConfig, run_host};

fn main() -> ExitCode {
    let result =
        HostConfig::for_current_user().and_then(|config| run_host(stdin(), stdout(), config));
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            // Standard output is reserved exclusively for framed messages.
            // Host errors are deliberately path-, URL-, and credential-free.
            eprintln!("download manager native host stopped: {error}");
            ExitCode::FAILURE
        }
    }
}
