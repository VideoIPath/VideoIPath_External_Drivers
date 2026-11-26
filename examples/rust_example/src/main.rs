//! Connect the API lessons to VideoIPath. Replace DemoDevice with your vendor adapter.
//! No simulated-device service or second protocol is part of this executable.
use rust_example::{
    application, config::Config, demo::DemoDevice, diagnostics, identity::NodeLock, protocol,
    registry::Registry, session, tls,
};
use std::{path::PathBuf, process::ExitCode, sync::Arc, time::Duration};
use tokio::{
    sync::mpsc,
    time::{Instant, timeout},
};
use tokio_tungstenite::{
    Connector, connect_async_tls_with_config, tungstenite::protocol::WebSocketConfig,
};
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> ExitCode {
    std::panic::set_hook(Box::new(|_| {
        eprintln!("internal invariant failure (details omitted)")
    }));
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = args.first().map(String::as_str).unwrap_or("help");
    if args.len() > 1 {
        eprintln!("unexpected arguments; use --help");
        return ExitCode::FAILURE;
    }
    match mode {
        "--version" => {
            println!("rust-example {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        "help" | "--help" => {
            println!(
                "rust-example: driver | check-config | health | status | --version\nAPI teaching example with in-memory state. Configuration: .env.example.\nOffline lessons: cargo run --locked --example api_walkthrough"
            );
            return ExitCode::SUCCESS;
        }
        "check-config" => {
            // Use the same parser as startup, without claiming the node or connecting.
            return match Config::environment(true) {
                Ok(_) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("{error}");
                    ExitCode::FAILURE
                }
            };
        }
        "health" | "status" => {
            // Local, bounded, side-effect-free; neither command needs credentials.
            let state = std::env::var_os("DRIVER_STATE_DIRECTORY")
                .map(PathBuf::from)
                .unwrap_or_else(|| "/var/lib/rust-example".into());
            return if mode == "health" {
                if diagnostics::healthy(&state) {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::FAILURE
                }
            } else {
                match diagnostics::status(&state) {
                    Ok(status) => {
                        println!("{status}");
                        ExitCode::SUCCESS
                    }
                    Err(_) => {
                        eprintln!("readiness not yet available");
                        ExitCode::FAILURE
                    }
                }
            };
        }
        "driver" => {}
        _ => {
            eprintln!("unknown command; use --help");
            return ExitCode::FAILURE;
        }
    }
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(cause) => {
            eprintln!("{cause}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), &'static str> {
    let config = Arc::new(
        Config::environment(true).map_err(|_| "invalid configuration; check .env.example")?,
    );
    // Never enable dependency TRACE logging: it can expose authentication headers.
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(format!("rust_example={}", config.log_level))
        .init();
    let registry = Arc::new(
        Registry::new()
            .with_configuration_order(&config.configuration_order)
            .map_err(|_| "invalid configuration order")?,
    );
    registry
        .validate()
        .map_err(|_| "invalid example structure")?;
    let _lock = NodeLock::acquire(
        &config.lock_directory,
        config.node_id.expect("validated identity"),
    )
    .map_err(|_| "node ownership unavailable; check singleton and lock-directory permissions")?;
    let connector = tls::connector(&config).map_err(|_| "invalid TLS configuration")?;
    std::fs::create_dir_all(&config.state_directory).map_err(|_| "state directory unavailable")?;

    // Adaptation point: construct/authenticate your vendor adapter here. Demo
    // state lives across WebSocket reconnects, but deliberately resets on restart.
    let device = Arc::new(DemoDevice::new(registry.clone()));
    let stop = CancellationToken::new();
    let result = supervised(
        supervise(config.clone(), registry, device, connector, stop.clone()),
        config.clone(),
        stop,
    )
    .await;
    let _ = std::fs::remove_file(config.state_directory.join("heartbeat"));
    let _ = diagnostics::readiness(&config.state_directory, false, false);
    result
}

/// Liveness is event-loop progress, not proof of backend or physical-device readiness.
async fn supervised<F>(
    run: F,
    config: Arc<Config>,
    stop: CancellationToken,
) -> Result<(), &'static str>
where
    F: std::future::Future<Output = Result<(), &'static str>>,
{
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .map_err(|_| "signal setup failed")?;
    let mut sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
        .map_err(|_| "signal setup failed")?;
    let mut tick = tokio::time::interval(Duration::from_secs(2));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    tokio::pin!(run);
    loop {
        tokio::select! {
            result=&mut run=>return result,
            _=tick.tick()=>diagnostics::heartbeat(&config.state_directory).map_err(|_|"heartbeat write failed")?,
            _=sigterm.recv()=>break,
            _=sigint.recv()=>break,
        }
    }
    stop.cancel();
    timeout(config.shutdown_timeout, &mut run)
        .await
        .map_err(|_| "shutdown deadline exceeded")?
}

fn handler(device: Arc<DemoDevice>, registry: Arc<Registry>) -> session::Handler {
    Arc::new(move |value| {
        let (device, registry) = (device.clone(), registry.clone());
        Box::pin(async move {
            match serde_json::from_value::<protocol::Request>(value.clone()) {
                Ok(request) => {
                    let done = application::handle(device.as_ref(), &registry, request).await;
                    session::Completion {
                        response: done.response,
                        changes: done.changes.iter().map(protocol::event_frame).collect(),
                    }
                }
                Err(_) => session::Completion {
                    response: serde_json::json!({"message_id":value["message_id"],"message_type":"error","data":"invalid request"}),
                    changes: vec![],
                },
            }
        })
    })
}

async fn supervise(
    config: Arc<Config>,
    registry: Arc<Registry>,
    device: Arc<DemoDevice>,
    connector: Arc<rustls::ClientConfig>,
    stop: CancellationToken,
) -> Result<(), &'static str> {
    let mut attempt = 0u32;
    loop {
        if stop.is_cancelled() {
            return Ok(());
        }
        if !device.healthy() {
            return Err("demo state unavailable");
        }
        let _ = diagnostics::readiness(&config.state_directory, true, false);
        let start = Instant::now();
        let request = tls::request(&config).map_err(|_| "invalid WebSocket credentials")?;
        let ws_config = WebSocketConfig::default()
            .max_message_size(Some(config.max_frame_bytes))
            .max_frame_size(Some(config.max_frame_bytes));
        let opened = tokio::select! {
            _=stop.cancelled()=>return Ok(()),
            result=timeout(config.request_timeout, connect_async_tls_with_config(request, Some(ws_config), true,
                Some(Connector::Rustls(connector.clone()))))=>result,
        };
        match opened {
            Ok(Ok((socket, _))) => {
                let _ = diagnostics::readiness(&config.state_directory, true, true);
                tracing::info!("VideoIPath session established (in-memory demonstration device)");
                let generation = stop.child_token();
                let (send, receive) = mpsc::channel(config.outbound_limit);
                let (event_device, event_stop) = (device.clone(), generation.clone());
                // Teaching input only. A real adapter supplies vendor events instead.
                // The read-only signal cannot be set by a request; no writable-value
                // echo reconciliation or invented vendor revision protocol is needed.
                let events = tokio::spawn(async move {
                    let mut tick = tokio::time::interval_at(
                        Instant::now() + Duration::from_secs(10),
                        Duration::from_secs(10),
                    );
                    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                    loop {
                        tokio::select! {
                            _=event_stop.cancelled()=>break,
                            _=tick.tick()=> {
                                let Ok(changes) = event_device.tick() else { break };
                                for change in changes {
                                    if send.try_send(protocol::event_frame(&change)).is_err() { return; }
                                }
                            }
                        }
                    }
                });
                let healthy = device.clone();
                let reason = session::run_independent(
                    socket,
                    handler(device.clone(), registry.clone()),
                    receive,
                    Arc::new(move || healthy.healthy()),
                    session::Limits {
                        requests: config.request_limit,
                        outbound: config.outbound_limit,
                        max_frame: config.max_frame_bytes,
                        ordinary: config.request_timeout,
                        ipt: config.ipt_timeout,
                    },
                    generation.clone(),
                )
                .await;
                generation.cancel();
                let _ = events.await;
                let _ = diagnostics::readiness(&config.state_directory, device.healthy(), false);
                tracing::warn!(reason, "session ended");
                if start.elapsed() > Duration::from_secs(60) {
                    attempt = 0;
                }
            }
            Ok(Err(error)) if session::permanent(&error) => {
                return Err("permanent authentication, URL, or TLS failure");
            }
            _ => tracing::warn!("VideoIPath connection unavailable"),
        }
        let delay = session::backoff(
            attempt,
            config.reconnect_min,
            config.reconnect_max,
            rand::random(),
        );
        attempt = attempt.saturating_add(1);
        tracing::info!(
            reconnect_count = attempt,
            delay_ms = delay.as_millis() as u64,
            "reconnect scheduled"
        );
        tokio::select! { _=stop.cancelled()=>return Ok(()), _=tokio::time::sleep(delay)=>{} }
    }
}
