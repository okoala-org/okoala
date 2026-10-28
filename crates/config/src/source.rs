// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! Secret references and reading the values they name.
//!
//! Buffers that held a secret and are discarded on an error path are dropped inside a `Secret`, so they are
//! zeroized. Messages name the file or variable, never the value.

use std::fs::File;
use std::io::{ErrorKind, Read};
use std::path::Path;

use okoala_secret::Secret;
use toml::Value;

use crate::env::{Environment, is_override_name};

/// The largest secret accepted, in bytes.
const MAX_SECRET_LEN: u64 = 64 * 1024;

/// The key of a reference to a secret file: `{ secret_file = "<path>" }`.
pub(crate) const FILE_KEY: &str = "secret_file";
/// The key of a reference to a secret environment variable: `{ secret_env = "<VARIABLE>" }`.
pub(crate) const ENV_KEY: &str = "secret_env";

const REFERENCE_FORM: &str = "a secret is given as { secret_file = \"<path>\" } or \
     { secret_env = \"<VARIABLE>\" }, never inline";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SecretKind {
    Text,
    Bytes,
}

/// A loaded secret value, handed straight to the field's visitor, which wraps it in a `Secret`.
pub(crate) enum Raw {
    Text(String),
    Bytes(Vec<u8>),
}

/// Where a secret is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Reference {
    File(String),
    Env(String),
}

impl Reference {
    fn origin(&self) -> String {
        match self {
            Self::File(path) => format!("secret file {path}"),
            Self::Env(name) => format!("environment variable {name}"),
        }
    }
}

pub(crate) fn discard_bytes(bytes: Vec<u8>) {
    drop(Secret::new(bytes));
}

fn discard_text(text: String) {
    drop(Secret::new(text));
}

/// The reference `value` is, if it has the exact shape of one (a table with the single key `secret_file` or
/// `secret_env` and a string value).
pub(crate) fn reference_of(value: &Value) -> Option<Reference> {
    let Value::Table(table) = value else {
        return None;
    };
    if table.len() != 1 {
        return None;
    }
    match (table.get(FILE_KEY), table.get(ENV_KEY)) {
        (Some(Value::String(path)), None) => Some(Reference::File(path.clone())),
        (None, Some(Value::String(name))) => Some(Reference::Env(name.clone())),
        _ => None,
    }
}

/// The reference a secret field's value must be; anything else is an error that does not quote the value.
pub(crate) fn parse_reference(value: Value) -> Result<Reference, String> {
    if let Some(reference) = reference_of(&value) {
        return Ok(reference);
    }
    match value {
        Value::String(inline) => {
            discard_text(inline);
            Err(format!(
                "inline secret values are not allowed; {REFERENCE_FORM}"
            ))
        }
        _ => Err(REFERENCE_FORM.to_owned()),
    }
}

/// Turns the bytes of a fetched secret into the field's kind. Text secrets lose one trailing newline (`\n` or
/// `\r\n`) and must then be one non-blank line; byte secrets are taken as they are.
pub(crate) fn finish(
    bytes: Vec<u8>,
    kind: SecretKind,
    reference: &Reference,
) -> Result<Raw, String> {
    let origin = reference.origin();
    match kind {
        SecretKind::Bytes => Ok(Raw::Bytes(bytes)),
        SecretKind::Text => {
            let mut text = match String::from_utf8(bytes) {
                Ok(text) => text,
                Err(e) => {
                    discard_bytes(e.into_bytes());
                    return Err(format!("{origin} is not valid UTF-8"));
                }
            };
            if text.ends_with('\n') {
                text.pop();
                if text.ends_with('\r') {
                    text.pop();
                }
            }
            let problem = if text.trim().is_empty() {
                Some("is empty or only whitespace")
            } else if text.contains(['\n', '\r']) {
                Some(
                    "contains a line break; a text secret is a single line (one trailing newline is \
                     allowed)",
                )
            } else {
                None
            };
            if let Some(problem) = problem {
                discard_text(text);
                return Err(format!("{origin} {problem}"));
            }
            Ok(Raw::Text(text))
        }
    }
}

pub(crate) struct SecretSource<'a> {
    pub(crate) env: &'a dyn Environment,
    pub(crate) kubernetes_dir: &'a Path,
}

impl SecretSource<'_> {
    /// Reads the bytes a reference names. Empty values are errors.
    pub(crate) fn fetch(&self, reference: &Reference) -> Result<Vec<u8>, String> {
        let origin = reference.origin();
        let bytes = match reference {
            Reference::File(path) => self.read_file(Path::new(path), &origin)?,
            Reference::Env(name) => {
                if is_override_name(name) {
                    return Err(format!(
                        "{origin} has the form of a configuration override (OKOALA_<SECTION>__<KEY>), so the \
                         loader would apply it as one; name the secret's variable without `__`"
                    ));
                }
                match self.env.var(name) {
                    Some(value) => value.into_bytes(),
                    None => return Err(format!("{origin} is not set or not valid UTF-8")),
                }
            }
        };
        if bytes.is_empty() {
            return Err(format!("{origin} is empty"));
        }
        Ok(bytes)
    }

    fn read_file(&self, path: &Path, origin: &str) -> Result<Vec<u8>, String> {
        let (mut file, under_kubernetes_dir) = self.open(path, origin)?;
        // Checked again on the opened file, which is the object actually read.
        let meta = file
            .metadata()
            .map_err(|e| format!("cannot inspect {origin}: {e}"))?;
        if !meta.is_file() {
            return Err(format!("{origin} is not a regular file"));
        }
        check_mode(&meta, under_kubernetes_dir, self.kubernetes_dir, origin)?;
        if meta.len() > MAX_SECRET_LEN {
            return Err(format!("{origin} is larger than {MAX_SECRET_LEN} bytes"));
        }
        // Read into a buffer allocated once, one byte larger than the file, so it never reallocates (which
        // would leave a copy of the secret in freed memory) and a file that grew since `metadata` is detected.
        let capacity = usize::try_from(meta.len())
            .map_err(|_| format!("{origin} is too large"))?
            .saturating_add(1);
        let mut buf = vec![0_u8; capacity];
        let mut filled = 0_usize;
        let outcome = loop {
            let rest: &mut [u8] = buf.get_mut(filled..).unwrap_or_default();
            if rest.is_empty() {
                break Err(format!("{origin} changed while it was read"));
            }
            match file.read(rest) {
                Ok(0) => break Ok(()),
                Ok(n) => filled = filled.saturating_add(n),
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => break Err(format!("cannot read {origin}: {e}")),
            }
        };
        if let Err(message) = outcome {
            discard_bytes(buf);
            return Err(message);
        }
        buf.truncate(filled);
        Ok(buf)
    }

    /// Opens a secret file, and says whether the opened file lies under the Kubernetes Secrets directory.
    ///
    /// The path is resolved once; only a regular file is opened (opening a FIFO would block), without
    /// following a final symlink and non-blocking, so a file swapped in after the check cannot hang the
    /// process. On Linux the location is then taken from the opened descriptor itself, so the location decision
    /// and the file read are the same object.
    #[cfg(unix)]
    fn open(&self, path: &Path, origin: &str) -> Result<(File, bool), String> {
        use rustix::fs::{Mode, OFlags};

        let canonical =
            std::fs::canonicalize(path).map_err(|e| format!("cannot open {origin}: {e}"))?;
        let meta =
            std::fs::metadata(&canonical).map_err(|e| format!("cannot inspect {origin}: {e}"))?;
        if !meta.is_file() {
            return Err(format!("{origin} is not a regular file"));
        }
        let flags =
            OFlags::RDONLY | OFlags::NONBLOCK | OFlags::NOFOLLOW | OFlags::NOCTTY | OFlags::CLOEXEC;
        let file = rustix::fs::open(&canonical, flags, Mode::empty())
            .map(File::from)
            .map_err(|e| format!("cannot open {origin}: {e}"))?;
        let location = opened_path(&file).unwrap_or(canonical);
        let under =
            std::fs::canonicalize(self.kubernetes_dir).is_ok_and(|dir| location.starts_with(dir));
        Ok((file, under))
    }

    #[cfg(not(unix))]
    fn open(&self, path: &Path, origin: &str) -> Result<(File, bool), String> {
        let file = File::open(path).map_err(|e| format!("cannot open {origin}: {e}"))?;
        Ok((file, false))
    }
}

/// The path of an open file, from `/proc/self/fd`.
#[cfg(target_os = "linux")]
fn opened_path(file: &File) -> Option<std::path::PathBuf> {
    use std::os::fd::AsRawFd;
    std::fs::read_link(format!("/proc/self/fd/{}", file.as_raw_fd())).ok()
}

#[cfg(all(unix, not(target_os = "linux")))]
fn opened_path(_file: &File) -> Option<std::path::PathBuf> {
    None
}

/// Mode 0400 or 0440; 0444 only under the Kubernetes Secrets directory. Unix only: elsewhere there are no
/// mode bits to check.
#[cfg(unix)]
fn check_mode(
    meta: &std::fs::Metadata,
    under_kubernetes_dir: bool,
    kubernetes_dir: &Path,
    origin: &str,
) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    let mode = meta.permissions().mode() & 0o7777;
    let dir = kubernetes_dir.display();
    match mode {
        0o400 | 0o440 => Ok(()),
        0o444 if under_kubernetes_dir => Ok(()),
        0o444 => Err(format!(
            "{origin} has mode 0444; a world-readable secret file is accepted only under {dir} \
             (Kubernetes Secret mounts); use mode 0400 or 0440"
        )),
        other => Err(format!(
            "{origin} has mode {other:04o}; a secret file must have mode 0400 or 0440 \
             (or 0444 under {dir})"
        )),
    }
}

#[cfg(not(unix))]
fn check_mode(
    _meta: &std::fs::Metadata,
    _under_kubernetes_dir: bool,
    _kubernetes_dir: &Path,
    _origin: &str,
) -> Result<(), String> {
    Ok(())
}
