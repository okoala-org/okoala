// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! Reading secret values from the file or environment variable a secret reference names.
//!
//! Buffers that held a secret and are discarded on an error path are dropped inside a `Secret`, so they are
//! zeroized. Messages name the file or variable, never the value.

use std::fs::File;
use std::io::{ErrorKind, Read};
use std::path::Path;

use okoala_secret::Secret;
use toml::Value;

use crate::env::Environment;

/// The largest secret accepted, in bytes.
const MAX_SECRET_LEN: u64 = 64 * 1024;

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

enum Reference {
    File(String),
    Env(String),
}

const REFERENCE_FORM: &str =
    "a secret is given as { file = \"<path>\" } or { env = \"<VARIABLE>\" }, never inline";

fn discard_bytes(bytes: Vec<u8>) {
    drop(Secret::new(bytes));
}

fn discard_text(text: String) {
    drop(Secret::new(text));
}

fn parse_reference(value: Value) -> Result<Reference, String> {
    match value {
        Value::String(inline) => {
            discard_text(inline);
            Err(format!(
                "inline secret values are not allowed; {REFERENCE_FORM}"
            ))
        }
        Value::Table(table) => {
            let mut entries = table.into_iter();
            match (entries.next(), entries.next()) {
                (Some((key, Value::String(target))), None) if key == "file" => {
                    Ok(Reference::File(target))
                }
                (Some((key, Value::String(target))), None) if key == "env" => {
                    Ok(Reference::Env(target))
                }
                _ => Err(REFERENCE_FORM.to_owned()),
            }
        }
        _ => Err(REFERENCE_FORM.to_owned()),
    }
}

pub(crate) struct SecretSource<'a> {
    pub(crate) env: &'a dyn Environment,
    pub(crate) kubernetes_dir: &'a Path,
}

impl SecretSource<'_> {
    pub(crate) fn load(&self, value: Value, kind: SecretKind) -> Result<Raw, String> {
        let (bytes, origin) = match parse_reference(value)? {
            Reference::File(path) => {
                let origin = format!("secret file {path}");
                (self.read_file(Path::new(&path), &origin)?, origin)
            }
            Reference::Env(name) => {
                let origin = format!("environment variable {name}");
                match self.env.var(&name) {
                    Some(value) => (value.into_bytes(), origin),
                    None => return Err(format!("{origin} is not set or not valid UTF-8")),
                }
            }
        };
        match kind {
            SecretKind::Bytes => {
                if bytes.is_empty() {
                    return Err(format!("{origin} is empty"));
                }
                Ok(Raw::Bytes(bytes))
            }
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
                if text.is_empty() {
                    discard_text(text);
                    return Err(format!("{origin} is empty"));
                }
                Ok(Raw::Text(text))
            }
        }
    }

    fn read_file(&self, path: &Path, origin: &str) -> Result<Vec<u8>, String> {
        let mut file = File::open(path).map_err(|e| format!("cannot open {origin}: {e}"))?;
        let meta = file
            .metadata()
            .map_err(|e| format!("cannot inspect {origin}: {e}"))?;
        if !meta.is_file() {
            return Err(format!("{origin} is not a regular file"));
        }
        self.check_mode(path, &meta, origin)?;
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

    #[cfg(unix)]
    fn check_mode(
        &self,
        path: &Path,
        meta: &std::fs::Metadata,
        origin: &str,
    ) -> Result<(), String> {
        use std::os::unix::fs::PermissionsExt;

        let mode = meta.permissions().mode() & 0o7777;
        let dir = self.kubernetes_dir.display();
        match mode {
            0o400 | 0o440 => Ok(()),
            0o444 if self.under_kubernetes_dir(path) => Ok(()),
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
        &self,
        _path: &Path,
        _meta: &std::fs::Metadata,
        _origin: &str,
    ) -> Result<(), String> {
        Ok(())
    }

    /// Whether the file, after resolving symlinks, lies under the Kubernetes Secrets directory (also
    /// resolved). A symlink from that directory to a file elsewhere does not count.
    #[cfg(unix)]
    fn under_kubernetes_dir(&self, path: &Path) -> bool {
        match (
            std::fs::canonicalize(path),
            std::fs::canonicalize(self.kubernetes_dir),
        ) {
            (Ok(file), Ok(dir)) => file.starts_with(dir),
            _ => false,
        }
    }
}
