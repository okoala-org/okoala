// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! Runs the `okoala` binary as a process, with log capture and the log-scrubbing check (implementation plan
//! §4.20, invariant I10).
//!
//! Every test that starts a server process goes through [`TestConfig`] and [`OkoalaProcess`]:
//!
//! ```ignore
//! let mut config = TestConfig::new("my-test");
//! let password = config.secret_file("smtp-relay-password"); // a known test secret, in a 0400 file
//! config.append_toml(&format!("[relay]\npassword = {{ secret_file = '{}' }}\n", password.display()));
//! let okoala = config.start(); // waits until the internal listener is up
//! let (status, _) = okoala.get("/healthz", &[]);
//! assert_eq!(status, 200);
//! let finished = okoala.stop(); // SIGTERM; fails if any known secret is in stdout or stderr
//! assert!(finished.status.success());
//! ```
//!
//! Every secret the helper writes (the internal token and each [`TestConfig::secret_file`]) and every value
//! passed to [`TestConfig::known_secret`] (a wrong token a test sends, say) is a known secret: a random value
//! no log could contain by chance. [`OkoalaProcess::stop`] fails the test if any of them, or any
//! [`WINDOW`]-character piece of one, appears in the captured output, so a truncated or partial leak is caught
//! too. The process is killed if the test panics first.
//!
//! The real `okoala-testkit` (WP 0.7) takes this over; until then, tests include this module.

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::hash::{BuildHasher, Hasher, RandomState};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// The length of the pieces of known secrets searched for in the output.
pub const WINDOW: usize = 12;

/// How long the process may take to start, and to stop after SIGTERM.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const STOP_TIMEOUT: Duration = Duration::from_secs(30);

/// A random value made of 32 lowercase hex digits.
pub fn random_value() -> String {
    let mut out = String::new();
    for _ in 0..2 {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        );
        hasher.write_u32(std::process::id());
        out.push_str(&format!("{:016x}", hasher.finish()));
    }
    out
}

/// The configuration of one test process: a temporary directory, the TOML text, and the known secrets.
pub struct TestConfig {
    dir: PathBuf,
    toml: String,
    env: Vec<(String, String)>,
    secrets: Vec<String>,
    token: String,
}

impl TestConfig {
    /// A configuration in a fresh directory for test `name`: the internal listener on `127.0.0.1:0` with a
    /// known token from a 0400 file, and every log event (`filter = "trace"`), so that the scrubbing check
    /// sees as much output as there can be.
    pub fn new(name: &str) -> Self {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("okoala-process-tests")
            .join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut config = Self {
            dir,
            toml: String::new(),
            env: Vec::new(),
            secrets: Vec::new(),
            token: String::new(),
        };
        let token_file = config.secret_file("internal-token");
        config.token = config.secrets[0].clone();
        config.toml = format!(
            "[internal]\nlisten = \"127.0.0.1:0\"\ntoken = {{ secret_file = '{}' }}\n\n\
             [telemetry]\nfilter = \"trace\"\n\n",
            token_file.display()
        );
        config
    }

    /// The test's temporary directory.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The internal token (a known secret).
    pub fn internal_token(&self) -> &str {
        &self.token
    }

    /// Writes a new known secret to a 0400 file in the test directory and returns the file's path.
    pub fn secret_file(&mut self, name: &str) -> PathBuf {
        let value = format!("{}{}", random_value(), random_value());
        let path = self.dir.join(name);
        std::fs::write(&path, format!("{value}\n")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o400)).unwrap();
        }
        self.secrets.push(value);
        path
    }

    /// Adds `value` to the known secrets (it must not appear in the output either).
    pub fn known_secret(&mut self, value: impl Into<String>) {
        self.secrets.push(value.into());
    }

    /// Appends TOML text (sections other than `[internal]` and `[telemetry]`, which [`TestConfig::new`] writes).
    pub fn append_toml(&mut self, text: &str) {
        self.toml.push_str(text);
    }

    /// Sets an environment variable for the process (whose environment is otherwise empty).
    pub fn env(&mut self, name: &str, value: &str) {
        self.env.push((name.to_owned(), value.to_owned()));
    }

    /// Writes the configuration and runs `okoala serve --config <file>`, waiting until the internal listener
    /// has started.
    pub fn start(self) -> OkoalaProcess {
        let config = self.dir.join("okoala.toml");
        std::fs::write(&config, &self.toml).unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_okoala"))
            .args(["serve", "--config"])
            .arg(&config)
            .env_clear()
            .envs(self.env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = Arc::new(Mutex::new(String::new()));
        let stderr = Arc::new(Mutex::new(String::new()));
        let (lines_tx, lines) = mpsc::channel();
        let readers = vec![
            capture(
                child.stdout.take().unwrap(),
                Arc::clone(&stdout),
                Some(lines_tx),
            ),
            capture(child.stderr.take().unwrap(), Arc::clone(&stderr), None),
        ];
        let mut process = OkoalaProcess {
            child: Some(child),
            internal_addr: None,
            stdout,
            stderr,
            readers,
            secrets: self.secrets,
            token: self.token,
        };
        process.internal_addr = Some(process.wait_for_listener(&lines));
        process
    }
}

/// Copies `from` into `into` line by line, forwarding each line to `lines` if given.
fn capture(
    from: impl Read + Send + 'static,
    into: Arc<Mutex<String>>,
    lines: Option<mpsc::Sender<String>>,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(from);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    into.lock().unwrap().push_str(&line);
                    if let Some(lines) = &lines {
                        let _ = lines.send(line.clone());
                    }
                }
            }
        }
    })
}

/// A running `okoala serve`.
pub struct OkoalaProcess {
    child: Option<Child>,
    internal_addr: Option<SocketAddr>,
    stdout: Arc<Mutex<String>>,
    stderr: Arc<Mutex<String>>,
    readers: Vec<JoinHandle<()>>,
    secrets: Vec<String>,
    token: String,
}

impl OkoalaProcess {
    fn wait_for_listener(&mut self, lines: &Receiver<String>) -> SocketAddr {
        let deadline = Instant::now() + STARTUP_TIMEOUT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match lines.recv_timeout(left) {
                Ok(line) => {
                    let Ok(event) = serde_json::from_str::<serde_json::Value>(&line) else {
                        continue;
                    };
                    if event["message"] == "internal listener started" {
                        return event["addr"].as_str().unwrap().parse().unwrap();
                    }
                }
                Err(_) => {
                    // Exited, or too slow: stop it and show what it said.
                    let finished = self.finish_with(|child| {
                        let _ = child.kill();
                    });
                    panic!(
                        "okoala did not start its internal listener ({:?})\nstdout:\n{}\nstderr:\n{}",
                        finished.status, finished.stdout, finished.stderr
                    );
                }
            }
        }
    }

    /// The internal listener's address.
    pub fn internal_addr(&self) -> SocketAddr {
        self.internal_addr.unwrap()
    }

    /// The internal token.
    pub fn internal_token(&self) -> &str {
        &self.token
    }

    /// Sends `raw` to the internal listener on a new connection and returns the whole answer (the server
    /// closes the connection after it, given `Connection: close` or an error).
    pub fn request(&self, raw: &[u8]) -> String {
        let mut stream = TcpStream::connect(self.internal_addr()).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        stream.write_all(raw).unwrap();
        let mut out = Vec::new();
        let _ = stream.read_to_end(&mut out);
        String::from_utf8_lossy(&out).into_owned()
    }

    /// `GET path` with `headers` (full lines without CRLF); returns the status and the body.
    pub fn get(&self, path: &str, headers: &[&str]) -> (u16, String) {
        let mut request = format!("GET {path} HTTP/1.1\r\nHost: okoala\r\nConnection: close\r\n");
        for h in headers {
            request.push_str(h);
            request.push_str("\r\n");
        }
        request.push_str("\r\n");
        let response = self.request(request.as_bytes());
        (status_of(&response), body_of(&response).to_owned())
    }

    /// Sends SIGTERM, waits for the process to exit, and runs the log-scrubbing check.
    pub fn stop(self) -> Finished {
        self.stop_with_signal(Signal::Term)
    }

    /// Like [`OkoalaProcess::stop`] with another signal.
    pub fn stop_with_signal(mut self, signal: Signal) -> Finished {
        let finished = self.finish_with(|child| send(child, signal));
        finished.assert_no_known_secret();
        finished
    }

    /// Signals the child (through `stop`), waits for it (killing it after [`STOP_TIMEOUT`]) and for the output.
    fn finish_with(&mut self, stop: impl FnOnce(&mut Child)) -> Finished {
        let mut child = self.child.take().expect("already stopped");
        stop(&mut child);
        let deadline = Instant::now() + STOP_TIMEOUT;
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() > deadline {
                let _ = child.kill();
                let status = child.wait().unwrap();
                panic!("okoala did not exit within {STOP_TIMEOUT:?} of being stopped ({status})");
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        for reader in self.readers.drain(..) {
            let _ = reader.join();
        }
        Finished {
            status,
            stdout: self.stdout.lock().unwrap().clone(),
            stderr: self.stderr.lock().unwrap().clone(),
            secrets: std::mem::take(&mut self.secrets),
        }
    }
}

impl Drop for OkoalaProcess {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// A signal [`OkoalaProcess::stop_with_signal`] can send.
#[derive(Debug, Clone, Copy)]
pub enum Signal {
    Term,
    Int,
}

#[cfg(unix)]
fn send(child: &mut Child, signal: Signal) {
    use rustix::process::{Pid, Signal as S, kill_process};
    let pid = Pid::from_child(child);
    let signal = match signal {
        Signal::Term => S::TERM,
        Signal::Int => S::INT,
    };
    kill_process(pid, signal).unwrap();
}

#[cfg(not(unix))]
fn send(child: &mut Child, _signal: Signal) {
    let _ = child.kill();
}

/// The output of a stopped process.
pub struct Finished {
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
    secrets: Vec<String>,
}

impl Finished {
    /// Fails if a known secret, or any [`WINDOW`]-character piece of one, is in stdout or stderr.
    pub fn assert_no_known_secret(&self) {
        for (stream, text) in [("stdout", &self.stdout), ("stderr", &self.stderr)] {
            for secret in &self.secrets {
                if let Some(piece) = leaked_piece(secret, text) {
                    panic!(
                        "a known test secret leaked to {stream} (the piece {piece:?} of a {}-character \
                         secret)\n{stream}:\n{text}",
                        secret.len()
                    );
                }
            }
        }
    }

    /// Stdout as JSON log events; fails on a line that is not a JSON object.
    pub fn log_events(&self) -> Vec<serde_json::Value> {
        self.stdout
            .lines()
            .map(|line| {
                let event: serde_json::Value = serde_json::from_str(line)
                    .unwrap_or_else(|e| panic!("not a JSON log line ({e}): {line}"));
                assert!(event.is_object(), "not a JSON object: {line}");
                event
            })
            .collect()
    }
}

/// The first piece of `secret` (the whole of it if it is short) found in `text`.
pub fn leaked_piece<'s>(secret: &'s str, text: &str) -> Option<&'s str> {
    if secret.len() <= WINDOW {
        return text.contains(secret).then_some(secret);
    }
    (0..=secret.len() - WINDOW)
        .filter_map(|i| secret.get(i..i + WINDOW))
        .find(|piece| text.contains(piece))
}

/// The status code of a raw HTTP response, or 0 if there is none.
pub fn status_of(response: &str) -> u16 {
    response
        .split(' ')
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

/// The body of a raw HTTP response.
pub fn body_of(response: &str) -> &str {
    response.split_once("\r\n\r\n").map_or("", |(_, body)| body)
}
