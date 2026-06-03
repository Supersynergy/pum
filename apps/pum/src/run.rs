use std::process::{Command, Stdio};
use std::time::Duration;

pub const SUBPROCESS_TIMEOUT: u64 = 60;

/// Run a command with timeout; returns (exit_code, stdout, stderr).
/// Never panics on missing binary or non-zero exit.
pub fn run(argv: &[&str], timeout_secs: u64) -> (i32, String, String) {
    let Some((prog, args)) = argv.split_first() else {
        return (-1, String::new(), "empty argv".to_string());
    };

    // Use a thread + channel to implement timeout since std doesn't have it.
    let prog_owned = prog.to_string();
    let args_owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();

    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = Command::new(&prog_owned)
            .args(&args_owned)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output();
        // Receiver may have already timed out and dropped rx; ignoring the
        // send error is intentional — the caller has moved on.
        let _ = tx.send(result);
    });

    match rx.recv_timeout(Duration::from_secs(timeout_secs)) {
        Ok(Ok(out)) => {
            let code = out.status.code().unwrap_or(-1);
            let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
            let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
            (code, stdout, stderr)
        }
        Ok(Err(e)) => {
            let msg = e.to_string();
            // io::Error kind NotFound means binary missing
            (-1, String::new(), msg)
        }
        Err(_) => (
            -1,
            String::new(),
            format!("timeout after {}s", timeout_secs),
        ),
    }
}

/// Convenience: run with default 60s timeout.
pub fn run_default(argv: &[&str]) -> (i32, String, String) {
    run(argv, SUBPROCESS_TIMEOUT)
}

pub fn which_exists(name: &str) -> bool {
    which::which(name).is_ok()
}
