// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! `okoala serve`: load and validate the configuration, then run the process until it is told to stop.
//!
//! [`run`] initialises logging, starts the process's tasks in a `JoinSet` (for now only the internal
//! listener; each selected role's `run(ctx, shutdown)` joins it with the first role crate), and waits for
//! SIGTERM or SIGINT. Shutdown then marks the process not ready, cancels the shared shutdown token, and waits
//! up to [`SHUTDOWN_DEADLINE`] for every task to finish. A task that ends on its own before shutdown, with an
//! error, `Ok` or a panic, ends the process with an error (§4.1: Kubernetes restarts it).

use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use okoala_config::{Config, ConfigErrors, Environment, Loader, Registry, RegistryError};
use okoala_internal_api::{InternalApi, InternalSection, Limits, Readiness};
use okoala_telemetry::{Metrics, TelemetryError, TelemetrySection};
use tokio::net::TcpListener;
use tokio::task::{JoinError, JoinSet};
use tokio_util::sync::CancellationToken;

use crate::process::{self, Context};
use crate::roles::RoleDescriptor;

/// How long shutdown waits for every task to finish. Below Kubernetes' default 30 s grace period, so the
/// process exits on its own before it is killed.
pub const SHUTDOWN_DEADLINE: Duration = Duration::from_secs(20);

/// Why `serve` failed.
#[derive(Debug)]
pub enum ServeError {
    /// A requested role is not in the role table.
    UnknownRoles(Vec<String>),
    /// Two section descriptors conflict (a programming error in the role table).
    Registry(RegistryError),
    /// The configuration is invalid.
    Config(ConfigErrors),
    /// Logging could not be set up.
    Telemetry(TelemetryError),
    /// The async runtime or the signal handlers could not be set up.
    Runtime(io::Error),
    /// The internal listener could not bind its address.
    Bind {
        /// The configured address.
        addr: SocketAddr,
        /// Why.
        error: io::Error,
    },
    /// A task ended before shutdown, or failed during it.
    Task {
        /// The task's name.
        name: &'static str,
        /// How it ended.
        reason: String,
    },
    /// Tasks were still running when [`SHUTDOWN_DEADLINE`] passed; they were aborted.
    ShutdownDeadline,
}

impl fmt::Display for ServeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownRoles(names) => write!(
                f,
                "unknown role{} {}; this build has no roles yet",
                if names.len() == 1 { "" } else { "s" },
                names.join(", ")
            ),
            Self::Registry(e) => write!(f, "{e}"),
            Self::Config(e) => write!(f, "{e}"),
            Self::Telemetry(e) => write!(f, "{e}"),
            Self::Runtime(e) => write!(f, "cannot start the runtime: {e}"),
            Self::Bind { addr, error } => {
                write!(f, "internal listener: cannot listen on {addr}: {error}")
            }
            Self::Task { name, reason } => write!(f, "task {name} {reason}"),
            Self::ShutdownDeadline => write!(
                f,
                "tasks still running {} s after shutdown started; aborted",
                SHUTDOWN_DEADLINE.as_secs()
            ),
        }
    }
}

impl std::error::Error for ServeError {}

/// The registry for a process: the process sections, then every role's sections. Every role's sections are
/// registered, not only the selected roles', so one configuration file serves every process of an
/// installation and is validated as a whole.
pub fn registry(roles: &[RoleDescriptor]) -> Result<Registry, RegistryError> {
    let mut registry = Registry::new();
    for descriptor in process::sections()
        .into_iter()
        .chain(roles.iter().flat_map(|r| r.sections.iter().copied()))
    {
        registry.register(descriptor)?;
    }
    Ok(registry)
}

/// The roles `--roles` selects from `table` (all of them when `requested` is `None`).
pub fn select<'t>(
    table: &'t [RoleDescriptor],
    requested: Option<&[String]>,
) -> Result<Vec<&'t RoleDescriptor>, ServeError> {
    let Some(requested) = requested else {
        return Ok(table.iter().collect());
    };
    let unknown: Vec<String> = requested
        .iter()
        .filter(|name| !table.iter().any(|r| r.name == name.as_str()))
        .cloned()
        .collect();
    if !unknown.is_empty() {
        return Err(ServeError::UnknownRoles(unknown));
    }
    Ok(table
        .iter()
        .filter(|r| requested.iter().any(|name| name == r.name))
        .collect())
}

/// Loads and validates the configuration at `path` for `table`.
pub fn load(
    path: &Path,
    table: &[RoleDescriptor],
    env: &dyn Environment,
) -> Result<Config, ServeError> {
    let registry = registry(table).map_err(ServeError::Registry)?;
    Loader::new(&registry, env)
        .load_file(path)
        .map_err(ServeError::Config)
}

/// Runs the process for `config` and the `selected` roles until SIGTERM or SIGINT, then shuts down. Installs
/// the global log subscriber and panic hook first, so call it once per process.
pub fn run(config: Config, selected: &[&RoleDescriptor]) -> Result<(), Vec<ServeError>> {
    let telemetry = config
        .get::<TelemetrySection>()
        .cloned()
        .unwrap_or_default();
    okoala_telemetry::init(&telemetry).map_err(|e| vec![ServeError::Telemetry(e)])?;
    okoala_telemetry::install_panic_hook();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| vec![ServeError::Runtime(e)])?;
    let result = runtime.block_on(async {
        let signal = shutdown_signal().map_err(|e| vec![ServeError::Runtime(e)])?;
        serve_until(config, selected, signal).await
    });
    if let Err(errors) = &result {
        for e in errors {
            tracing::error!(error = %e, "okoala stopped with an error");
        }
    } else {
        tracing::info!("okoala stopped");
    }
    // Tasks still running were aborted above; do not wait on blocking threads for long.
    runtime.shutdown_timeout(Duration::from_secs(1));
    result
}

/// Like [`run`] without the global setup: runs until `stop` completes (its output names the reason, such as
/// the signal), then shuts down. The internal listener binds `[internal] listen`.
pub async fn serve_until(
    config: Config,
    selected: &[&RoleDescriptor],
    stop: impl Future<Output = &'static str>,
) -> Result<(), Vec<ServeError>> {
    let config = Arc::new(config);
    let ctx = Context {
        config: Arc::clone(&config),
        metrics: Arc::new(Metrics::new(env!("CARGO_PKG_VERSION"))),
        readiness: Arc::new(Readiness::new()),
    };
    let Some(internal) = config.get::<InternalSection>() else {
        // `process::sections()` always registers it, so the loader always produced it.
        return Err(vec![ServeError::Task {
            name: "internal-listener",
            reason: "has no [internal] section".to_owned(),
        }]);
    };
    // `Secret`'s `Debug` redacts, so the section is safe to log (and the log-scrubbing test checks it).
    tracing::debug!(section = ?internal, "internal listener configuration");
    let listener = TcpListener::bind(internal.listen).await.map_err(|error| {
        vec![ServeError::Bind {
            addr: internal.listen,
            error,
        }]
    })?;
    let addr = listener.local_addr().map_err(|error| {
        vec![ServeError::Bind {
            addr: internal.listen,
            error,
        }]
    })?;

    let shutdown = CancellationToken::new();
    let mut tasks = JoinSet::new();
    let mut names = HashMap::new();
    let api = Arc::new(InternalApi::new(
        Arc::clone(&ctx.config),
        Arc::clone(&ctx.metrics),
        Arc::clone(&ctx.readiness),
    ));
    let handle = tasks.spawn({
        let shutdown = shutdown.clone();
        async move {
            okoala_internal_api::serve(listener, api, Limits::default(), shutdown)
                .await
                .map_err(|e| e.to_string())
        }
    });
    names.insert(handle.id(), "internal-listener");
    tracing::info!(%addr, "internal listener started");
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        roles = %selected.iter().map(|r| r.name).collect::<Vec<_>>().join(","),
        sections = %config.section_names().collect::<Vec<_>>().join(","),
        "okoala started"
    );

    let mut errors = Vec::new();
    tokio::select! {
        reason = stop => tracing::info!(reason, "shutdown requested"),
        Some(joined) = tasks.join_next_with_id() => {
            // Before shutdown, a task ending at all is a failure.
            let (name, reason) = describe(&names, joined, "ended before shutdown");
            errors.push(ServeError::Task { name, reason });
        }
    }
    ctx.readiness.start_shutdown();
    shutdown.cancel();
    let drained = tokio::time::timeout(SHUTDOWN_DEADLINE, async {
        while let Some(joined) = tasks.join_next_with_id().await {
            if let Some(error) = during_shutdown(&names, joined) {
                errors.push(error);
            }
        }
    })
    .await;
    if drained.is_err() {
        tasks.abort_all();
        errors.push(ServeError::ShutdownDeadline);
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

type Joined = Result<(tokio::task::Id, Result<(), String>), JoinError>;

fn describe(
    names: &HashMap<tokio::task::Id, &'static str>,
    joined: Joined,
    ok: &str,
) -> (&'static str, String) {
    let name = |id| names.get(&id).copied().unwrap_or("(unknown)");
    match joined {
        Ok((id, Ok(()))) => (name(id), ok.to_owned()),
        Ok((id, Err(e))) => (name(id), format!("failed: {e}")),
        Err(e) if e.is_panic() => (name(e.id()), "panicked".to_owned()),
        Err(e) => (name(e.id()), "was cancelled".to_owned()),
    }
}

fn during_shutdown(
    names: &HashMap<tokio::task::Id, &'static str>,
    joined: Joined,
) -> Option<ServeError> {
    if matches!(joined, Ok((_, Ok(())))) {
        return None;
    }
    let (name, reason) = describe(names, joined, "");
    Some(ServeError::Task { name, reason })
}

/// Completes with the signal's name on the first SIGTERM or SIGINT. The handlers are installed when this is
/// called, before the returned future is polled, so a signal that arrives early is not missed.
#[cfg(unix)]
fn shutdown_signal() -> io::Result<impl Future<Output = &'static str>> {
    use tokio::signal::unix::{SignalKind, signal};
    let mut term = signal(SignalKind::terminate())?;
    let mut int = signal(SignalKind::interrupt())?;
    Ok(async move {
        tokio::select! {
            _ = term.recv() => "SIGTERM",
            _ = int.recv() => "SIGINT",
        }
    })
}

/// Not Unix: Ctrl-C only.
#[cfg(not(unix))]
fn shutdown_signal() -> io::Result<impl Future<Output = &'static str>> {
    Ok(async {
        let _ = tokio::signal::ctrl_c().await;
        "ctrl-c"
    })
}
