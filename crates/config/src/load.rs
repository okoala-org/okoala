// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! The loader: one TOML file, environment overrides, every registered section, all errors at once.

use std::any::Any;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

use toml::{Table, Value};

use crate::de::{Ctx, Fetched, ValueDe};
use crate::env::{Environment, OVERRIDE_PREFIX};
use crate::error::{ConfigError, ConfigErrors};
use crate::registry::{Parsed, Registry, Section, valid_name};
use crate::source::{Reference, SecretSource, discard_bytes, reference_of};

/// Where Kubernetes mounts Secrets; the only place a secret file may have mode 0444.
pub const DEFAULT_KUBERNETES_SECRETS_DIR: &str = "/run/secrets";

/// A loaded, validated configuration: one value per registered section.
pub struct Config {
    sections: BTreeMap<&'static str, Box<dyn Any + Send + Sync>>,
}

impl Config {
    /// The section of type `S`, or `None` if `S` was not registered with the loader's registry.
    pub fn get<S: Section>(&self) -> Option<&S> {
        self.sections.get(S::NAME)?.downcast_ref::<S>()
    }

    /// The names of the loaded sections, sorted.
    pub fn section_names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.sections.keys().copied()
    }
}

impl fmt::Debug for Config {
    // Section names only; each section is reached through `get` and has its own (redacting) `Debug`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("sections", &self.sections.keys().collect::<Vec<_>>())
            .finish()
    }
}

/// Loads configuration files against a registry.
pub struct Loader<'r> {
    registry: &'r Registry,
    env: &'r dyn Environment,
    kubernetes_secrets_dir: PathBuf,
}

impl fmt::Debug for Loader<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Loader")
            .field("registry", &self.registry)
            .field("kubernetes_secrets_dir", &self.kubernetes_secrets_dir)
            .finish_non_exhaustive()
    }
}

/// Which environment variable set a value, to point at it in errors.
struct Origin {
    section: String,
    key: String,
    variable: String,
}

impl<'r> Loader<'r> {
    /// A loader for the sections of `registry`, reading overrides and `{ secret_env = "..." }` secrets from `env`.
    pub fn new(registry: &'r Registry, env: &'r dyn Environment) -> Self {
        Self {
            registry,
            env,
            kubernetes_secrets_dir: PathBuf::from(DEFAULT_KUBERNETES_SECRETS_DIR),
        }
    }

    /// Replaces the directory under which secret files may have mode 0444 (default `/run/secrets`). For tests,
    /// which cannot create files there.
    #[must_use]
    pub fn with_kubernetes_secrets_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.kubernetes_secrets_dir = dir.into();
        self
    }

    /// Reads and loads the file at `path`.
    pub fn load_file(&self, path: &Path) -> Result<Config, ConfigErrors> {
        let text = std::fs::read_to_string(path).map_err(|e| {
            ConfigErrors::new(vec![ConfigError::general(format!(
                "cannot read configuration file {}: {e}",
                path.display()
            ))])
        })?;
        self.load_str(&text)
    }

    /// Loads configuration text.
    pub fn load_str(&self, text: &str) -> Result<Config, ConfigErrors> {
        let mut table = parse_toml(text).map_err(|e| ConfigErrors::new(vec![e]))?;
        let mut errors = Vec::new();
        // A variable the file names as a secret's `secret_env` is never applied as an override, even if it has
        // an override's shape (the secret loader reports that as an error), so its value is not copied into the
        // configuration.
        let mut secret_vars = BTreeSet::new();
        for (key, value) in &table {
            visit_references(value, &mut vec![key.clone()], &mut |_, reference| {
                if let Reference::Env(name) = reference {
                    secret_vars.insert(name);
                }
            });
        }
        let origins = self.apply_overrides(&mut table, &secret_vars, &mut errors);

        let ctx = Ctx {
            secrets: SecretSource {
                env: self.env,
                kubernetes_dir: &self.kubernetes_secrets_dir,
            },
            secret_errors: RefCell::new(Vec::new()),
            fetched: RefCell::new(Fetched::new()),
        };
        let mut sections: BTreeMap<&'static str, Parsed> = BTreeMap::new();
        for descriptor in self.registry.descriptors() {
            let name = descriptor.name();
            let value = table
                .remove(name)
                .unwrap_or_else(|| Value::Table(Table::new()));
            // Every secret reference in the section is fetched first, so that the ones deserialization never
            // reaches (it stops at a section's first structural error) are still checked and reported.
            let mut fetched = Fetched::new();
            visit_references(&value, &mut Vec::new(), &mut |path, reference| {
                let result = ctx.secrets.fetch(&reference);
                fetched.insert(path.to_vec(), (reference, result));
            });
            ctx.fetched.replace(fetched);
            let parsed = descriptor.parse(ValueDe::new(value, &ctx));
            for e in ctx.secret_errors.take() {
                errors.push(section_error(name, e.path, e.message, &origins));
            }
            for (path, (_, result)) in ctx.fetched.take() {
                match result {
                    Ok(bytes) => discard_bytes(bytes),
                    Err(message) => errors.push(section_error(name, Some(path), message, &origins)),
                }
            }
            match parsed {
                Ok(parsed) => {
                    sections.insert(name, parsed);
                }
                Err(e) => errors.push(section_error(name, e.path, e.message, &origins)),
            }
        }
        for name in table.keys() {
            errors.push(ConfigError::new(
                Some(name.clone()),
                None,
                format!("unknown section; known sections: {}", self.known_sections()),
            ));
        }
        if errors.is_empty() {
            Ok(Config { sections })
        } else {
            Err(ConfigErrors::new(errors))
        }
    }

    fn known_sections(&self) -> String {
        let names: Vec<_> = self.registry.names().collect();
        if names.is_empty() {
            "none".to_owned()
        } else {
            names.join(", ")
        }
    }

    fn apply_overrides(
        &self,
        table: &mut Table,
        secret_vars: &BTreeSet<String>,
        errors: &mut Vec<ConfigError>,
    ) -> Vec<Origin> {
        let mut names = self.env.names();
        names.sort();
        let mut origins = Vec::new();
        for variable in names {
            if secret_vars.contains(&variable) {
                continue;
            }
            let Some(rest) = variable.strip_prefix(OVERRIDE_PREFIX) else {
                continue;
            };
            if !rest.contains("__") {
                continue;
            }
            let segments: Vec<String> = rest.split("__").map(str::to_ascii_lowercase).collect();
            let Some((section, keys)) = segments.split_first() else {
                continue;
            };
            if segments.iter().any(|s| !valid_name(s)) {
                errors.push(ConfigError::general(format!(
                    "environment variable {variable}: an override is named OKOALA_<SECTION>__<KEY>[__<KEY>...], \
                     each part ASCII letters, digits and `_`"
                )));
                continue;
            }
            if !self.registry.contains(section) {
                errors.push(ConfigError::new(
                    Some(section.clone()),
                    None,
                    format!(
                        "unknown section (from environment variable {variable}); known sections: {}",
                        self.known_sections()
                    ),
                ));
                continue;
            }
            let Some(raw) = self.env.var(&variable) else {
                errors.push(ConfigError::general(format!(
                    "environment variable {variable} is not valid UTF-8"
                )));
                continue;
            };
            if let Err(message) = set_value(table, &segments, env_value(raw)) {
                errors.push(ConfigError::new(
                    Some(section.clone()),
                    Some(keys.join(".")),
                    format!("cannot apply environment variable {variable}: {message}"),
                ));
                continue;
            }
            origins.push(Origin {
                section: section.clone(),
                key: keys.join("."),
                variable,
            });
        }
        origins
    }
}

fn section_error(
    section: &str,
    path: Option<Vec<String>>,
    message: String,
    origins: &[Origin],
) -> ConfigError {
    let key = path.filter(|p| !p.is_empty()).map(|p| join_path(&p));
    let mut message = message;
    if let Some(key) = &key
        && let Some(origin) = origins.iter().find(|o| {
            o.section == section
                && (key == &o.key
                    || key.starts_with(&format!("{}.", o.key))
                    || key.starts_with(&format!("{}[", o.key)))
        })
    {
        message = format!(
            "{message} (value from environment variable {})",
            origin.variable
        );
    }
    ConfigError::new(Some(section.to_owned()), key, message)
}

fn join_path(path: &[String]) -> String {
    let mut out = String::new();
    for segment in path {
        if !out.is_empty() && !segment.starts_with('[') {
            out.push('.');
        }
        out.push_str(segment);
    }
    out
}

/// Parses the file. A syntax error is reported by position only: the parser's own rendering quotes the line,
/// which could be a secret written inline by mistake.
fn parse_toml(text: &str) -> Result<Table, ConfigError> {
    text.parse::<Table>().map_err(|e| {
        let position = e
            .span()
            .and_then(|span| text.get(..span.start))
            .map(|before| {
                let line = before.matches('\n').count().saturating_add(1);
                let column = before
                    .rsplit('\n')
                    .next()
                    .map_or(0, |l| l.chars().count())
                    .saturating_add(1);
                format!(" at line {line}, column {column}")
            })
            .unwrap_or_default();
        ConfigError::general(format!(
            "TOML syntax error{position}: {}",
            e.message().trim_end()
        ))
    })
}

/// An override's value: a TOML value if it parses as one (`9090`, `true`, `"text"`, `[1, 2]`), otherwise the
/// text itself.
fn env_value(raw: String) -> Value {
    match format!("value = {raw}").parse::<Table>() {
        Ok(mut parsed) if parsed.len() == 1 => parsed.remove("value").unwrap_or(Value::String(raw)),
        _ => Value::String(raw),
    }
}

fn set_value(table: &mut Table, segments: &[String], value: Value) -> Result<(), String> {
    let Some((last, parents)) = segments.split_last() else {
        return Err("empty key".to_owned());
    };
    let mut current = table;
    for (depth, segment) in parents.iter().enumerate() {
        let entry = current
            .entry(segment.clone())
            .or_insert_with(|| Value::Table(Table::new()));
        current = match entry {
            Value::Table(inner) => inner,
            _ => {
                let at = segments
                    .get(..=depth)
                    .map(|s| s.join("."))
                    .unwrap_or_default();
                return Err(format!("`{at}` is not a table in the configuration file"));
            }
        };
    }
    current.insert(last.clone(), value);
    Ok(())
}

/// Calls `f` with the key path and reference of every secret reference in `value`, with paths in the form the
/// deserializer uses (`key`, `[index]`).
fn visit_references(
    value: &Value,
    path: &mut Vec<String>,
    f: &mut dyn FnMut(&[String], Reference),
) {
    if let Some(reference) = reference_of(value) {
        f(path, reference);
        return;
    }
    match value {
        Value::Table(table) => {
            for (key, inner) in table {
                path.push(key.clone());
                visit_references(inner, path, f);
                path.pop();
            }
        }
        Value::Array(items) => {
            for (index, inner) in items.iter().enumerate() {
                path.push(format!("[{index}]"));
                visit_references(inner, path, f);
                path.pop();
            }
        }
        _ => {}
    }
}
