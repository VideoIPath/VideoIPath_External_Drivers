//! Composition root: concrete transports are constructed here, not in handlers.
use rust_example::{
    application, config::Config, device::Client, diagnostics, identity::NodeLock, protocol,
    registry::Registry, session, simulator, tls,
};
use std::{path::PathBuf, process::ExitCode, sync::Arc, time::Duration};
use tokio::{sync::mpsc, time::timeout};
use tokio_tungstenite::{
    Connector, connect_async_tls_with_config, tungstenite::protocol::WebSocketConfig,
};
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> ExitCode {
    // A panic must not dump an arbitrary device payload or secret-bearing model.
    std::panic::set_hook(Box::new(|_| {
        eprintln!("internal invariant failure (details omitted)")
    }));
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = args.first().map(String::as_str).unwrap_or("help");
    if mode == "--version" {
        println!("rust-example {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    if mode == "help" || mode == "--help" {
        println!(
            "rust-example: driver | simulator | simctl <command> [args] | health | status | --version\nConfiguration uses environment variables; see .env.example. No credentials are accepted as arguments."
        );
        return ExitCode::SUCCESS;
    }
    if mode == "health" || mode == "status" {
        // Health reads only a local progress marker. It does not parse or need credentials.
        let state = std::env::var_os("DRIVER_STATE_DIRECTORY")
            .map(PathBuf::from)
            .unwrap_or_else(|| "/var/lib/rust-example".into());
        if mode == "status" {
            return match diagnostics::status(&state) {
                Ok(status) => {
                    println!("{status}");
                    ExitCode::SUCCESS
                }
                Err(_) => {
                    eprintln!("readiness not yet available");
                    ExitCode::FAILURE
                }
            };
        }
        return if diagnostics::healthy(&state) {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        };
    }
    if !["driver", "simulator", "simctl"].contains(&mode) {
        eprintln!("unknown command; use --help");
        return ExitCode::FAILURE;
    }
    let config = match Config::environment(mode == "driver") {
        Ok(c) => Arc::new(c),
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    // Our own diagnostics are allowlisted. Dependency TRACE logging could include headers,
    // so the requested level applies only to this crate, not the TLS/WebSocket libraries.
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(format!("rust_example={}", config.log_level))
        .init();
    if mode == "simctl" {
        let Some(command) = args.get(1) else {
            eprintln!("simctl requires a command; see docs/SIMULATOR.md");
            return ExitCode::FAILURE;
        };
        return match simulator::control(&config.control_socket, command, &args[2..]).await {
            Ok(mut value) => {
                redact(&mut value);
                println!("{value}");
                ExitCode::SUCCESS
            }
            Err(_) => {
                eprintln!(
                    "simulator command failed; check command syntax and local socket permissions"
                );
                ExitCode::FAILURE
            }
        };
    }
    let registry = Arc::new(
        Registry::new()
            .with_configuration_order(&config.configuration_order)
            .expect("validated configuration order"),
    );
    if registry.validate().is_err() {
        eprintln!("invalid example structure");
        return ExitCode::FAILURE;
    }
    // Keep the guard in main's scope across every reconnect. A stale lock FILE is harmless.
    let _lock = if mode == "driver" {
        match NodeLock::acquire(
            &config.lock_directory,
            config.node_id.expect("validated node ID"),
        ) {
            Ok(lock) => Some(lock),
            Err(_) => {
                eprintln!(
                    "node ownership unavailable; check singleton and lock-directory permissions"
                );
                return ExitCode::FAILURE;
            }
        }
    } else {
        None
    };
    let connector = if mode == "driver" {
        match tls::connector(&config) {
            Ok(c) => Some(c),
            Err(e) => {
                eprintln!("{e}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        None
    };
    if std::fs::create_dir_all(&config.state_directory).is_err() {
        eprintln!("state directory unavailable");
        return ExitCode::FAILURE;
    }
    let stop = CancellationToken::new();
    let result = if mode == "driver" {
        supervised(
            supervise(config.clone(), registry, connector.unwrap(), stop.clone()),
            config.clone(),
            stop.clone(),
        )
        .await
    } else {
        supervised(
            async {
                simulator::serve(&config.socket, &config.control_socket, stop.clone())
                    .await
                    .map_err(|_| "simulator supervisor failed")
            },
            config.clone(),
            stop.clone(),
        )
        .await
    };
    let _ = std::fs::remove_file(config.state_directory.join("heartbeat"));
    let _ = diagnostics::readiness(&config.state_directory, false, false);
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(cause) => {
            tracing::error!(cause, "stopped");
            ExitCode::FAILURE
        }
    }
}

/// A heartbeat measures event-loop progress, while dependency readiness is reported separately.
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

async fn supervise(
    config: Arc<Config>,
    registry: Arc<Registry>,
    tls_config: Arc<rustls::ClientConfig>,
    stop: CancellationToken,
) -> Result<(), &'static str> {
    let mut attempt = 0u32;
    loop {
        if stop.is_cancelled() {
            return Ok(());
        }
        let _ = diagnostics::readiness(&config.state_directory, false, false);
        let start = tokio::time::Instant::now();
        let connected = tokio::select! {
            _=stop.cancelled()=>return Ok(()),
            result=timeout(config.operation_timeout,Client::connect(&config.socket,config.operation_timeout))=>result,
        };
        if let Ok(Ok(device)) = connected {
            // Subscribe before startup reads and the WebSocket handshake. Broadcast
            // overflow is then observable, rather than silently losing early edges.
            let mut updates = device.subscribe_observed();
            let _ = diagnostics::readiness(&config.state_directory, true, false);
            // Establish enough target state to support advertised parameter reads first.
            use rust_example::domain::{Device, Location};
            if device.read_values(Location::root()).await.is_ok() {
                let request = tls::request(&config).map_err(|_| "invalid WebSocket credentials")?;
                let ws_config = WebSocketConfig::default()
                    .max_message_size(Some(config.max_frame_bytes))
                    .max_frame_size(Some(config.max_frame_bytes));
                let opened = tokio::select! {
                    _=stop.cancelled()=>{device.close().await;return Ok(());},
                    result=timeout(config.request_timeout,connect_async_tls_with_config(request,Some(ws_config),true,Some(Connector::Rustls(tls_config.clone()))))=>result,
                };
                match opened {
                    Ok(Ok((socket, _))) => {
                        let _ = diagnostics::readiness(&config.state_directory, true, true);
                        tracing::info!("VideoIPath session established");
                        let device = Arc::new(device);
                        let handler_device = device.clone();
                        let handler_registry = registry.clone();
                        let handler: session::Handler = Arc::new(move |value| {
                            let device = handler_device.clone();
                            let registry = handler_registry.clone();
                            Box::pin(async move {
                                match serde_json::from_value::<protocol::Request>(value.clone()) {
                                    Ok(request) => {
                                        let done = application::handle(
                                            device.as_ref(),
                                            registry.as_ref(),
                                            request,
                                        )
                                        .await;
                                        session::Completion {
                                            response: done.response,
                                            changes: done
                                                .changes
                                                .iter()
                                                .map(protocol::event_frame)
                                                .collect(),
                                        }
                                    }
                                    Err(_) => session::Completion {
                                        response: serde_json::json!({"message_id":value["message_id"],"message_type":"error","data":"invalid request"}),
                                        changes: vec![],
                                    },
                                }
                            })
                        });
                        let (events_tx, events_rx) = mpsc::channel(config.outbound_limit);
                        let generation_stop = stop.child_token();
                        let event_stop = generation_stop.clone();
                        let events = tokio::spawn(async move {
                            loop {
                                tokio::select! {
                                    _=event_stop.cancelled()=>break,
                                    event=updates.recv()=>match event {
                                    Ok(observed)=> {
                                        let mut frame=protocol::event_frame(&observed.event);
                                        frame["_driver_revision"]=serde_json::json!(observed.revision);
                                        if events_tx.try_send(frame).is_err(){break;}
                                    },
                                        // A lagged event stream is not a fresh observation; its closure
                                        // forces session recovery rather than silently skipping edges.
                                        Err(_)=>break,
                                    }
                                }
                            }
                        });
                        let probe_device = device.clone();
                        let probe_stop = generation_stop.clone();
                        let probe = tokio::spawn(async move {
                            let mut tick = tokio::time::interval(Duration::from_secs(5));
                            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                            loop {
                                tokio::select! {
                                    _ = probe_stop.cancelled() => break,
                                    _ = tick.tick() => {
                                        if probe_device.probe().await.is_err() {
                                            probe_device.close().await;
                                            break;
                                        }
                                    }
                                }
                            }
                        });
                        let health_device = device.clone();
                        let event_device = device.clone();
                        let current = Arc::new(move |frame: &serde_json::Value| {
                            if frame["message_type"] != "async_values_changed" {
                                return true;
                            }
                            let Some(revision) = frame["_driver_revision"].as_u64() else {
                                return false;
                            };
                            let Ok(location) =
                                serde_json::from_value(frame["data"]["location"].clone())
                            else {
                                return false;
                            };
                            let Ok(values) =
                                serde_json::from_value(frame["data"]["values"].clone())
                            else {
                                return false;
                            };
                            event_device.event_is_current(&rust_example::device::ObservedEvent {
                                revision,
                                event: rust_example::domain::Event::Values { location, values },
                            })
                        });
                        let reason = session::run_guarded(
                            socket,
                            handler,
                            events_rx,
                            Arc::new(move || health_device.is_healthy()),
                            session::Limits {
                                requests: config.request_limit,
                                outbound: config.outbound_limit,
                                max_frame: config.max_frame_bytes,
                                ordinary: config.request_timeout,
                                ipt: config.ipt_timeout,
                            },
                            generation_stop.clone(),
                            current,
                        )
                        .await;
                        let _ = diagnostics::readiness(&config.state_directory, false, false);
                        generation_stop.cancel();
                        let _ = events.await;
                        let _ = probe.await;
                        device.close().await;
                        tracing::warn!(reason, "session ended");
                        // Reset only after a stable session. Immediate takeover/close must not
                        // defeat backoff and create a reconnect storm between duplicate owners.
                        if start.elapsed() > Duration::from_secs(60) {
                            attempt = 0;
                        }
                    }
                    Ok(Err(error)) if session::permanent(&error) => {
                        device.close().await;
                        return Err("permanent authentication, URL, or TLS failure");
                    }
                    _ => {
                        device.close().await;
                        tracing::warn!("VideoIPath connection unavailable");
                    }
                }
            } else {
                device.close().await;
            }
        } else {
            tracing::warn!("simulated device unavailable");
        }
        let _ = diagnostics::readiness(&config.state_directory, false, false);
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
        tokio::select! {_=stop.cancelled()=>return Ok(()),_=tokio::time::sleep(delay)=>{}}
    }
}

fn redact(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, value) in map {
                if ["key", "token", "password", "sdp", "encryption"].contains(&key.as_str()) {
                    *value = serde_json::Value::String("[redacted]".into());
                } else {
                    redact(value);
                }
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                redact(value);
            }
        }
        _ => {}
    }
}
