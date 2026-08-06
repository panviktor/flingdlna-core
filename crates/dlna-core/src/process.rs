//! Small helpers for running external commands safely.

use std::io::{self, Read};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub struct CommandStdout {
    pub success: bool,
    pub stdout: Vec<u8>,
}

fn read_to_end_limited<R: Read>(r: &mut R, limit: usize) -> io::Result<Vec<u8>> {
    let mut out: Vec<u8> = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let n = r.read(&mut buf)?;
        if n == 0 {
            break;
        }
        if out.len().saturating_add(n) > limit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Command stdout exceeded limit",
            ));
        }
        out.extend_from_slice(&buf[..n]);
    }
    Ok(out)
}

/// Run a command and capture stdout, with a timeout and an upper bound on captured bytes.
///
/// Returns `Ok(None)` on timeout (process is killed best-effort).
pub fn run_command_capture_stdout(
    program: &str,
    args: &[&str],
    timeout: Duration,
    max_stdout_bytes: usize,
) -> io::Result<Option<CommandStdout>> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;

    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait()? {
            Some(status) => {
                let stdout = match child.stdout.as_mut() {
                    Some(s) => read_to_end_limited(s, max_stdout_bytes)?,
                    None => Vec::new(),
                };
                return Ok(Some(CommandStdout {
                    success: status.success(),
                    stdout,
                }));
            }
            None => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Ok(None);
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }
}
