//! Repeatable local device-API measurement, not a claim about VideoIPath latency.
//! Run: cargo run --locked --release --features device-lab --example latency
use rust_example::{
    domain::{Device, Location},
    lab::{client::Client, simulator},
};
use std::os::unix::fs::PermissionsExt;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()?;
    let socket = dir.path().join("device.sock");
    let control = dir.path().join("control.sock");
    let stop = CancellationToken::new();
    let (device_path, control_path, server_stop) = (socket.clone(), control, stop.clone());
    let server =
        tokio::spawn(
            async move { simulator::serve(&device_path, &control_path, server_stop).await },
        );
    // Bounded readiness retry is outside the measured section, never a retry of a write.
    let deadline = Instant::now() + Duration::from_secs(5);
    let device = loop {
        match Client::connect(&socket, Duration::from_secs(1)).await {
            Ok(client) => break client,
            Err(_) if Instant::now() < deadline => tokio::task::yield_now().await,
            Err(error) => return Err(error.into()),
        }
    };
    for _ in 0..100 {
        device.read_values(Location::root()).await?;
    }
    let mut samples = Vec::with_capacity(2000);
    for _ in 0..2000 {
        let start = Instant::now();
        device.read_values(Location::root()).await?;
        samples.push(start.elapsed().as_micros());
    }
    samples.sort_unstable();
    println!(
        "workload=2000_sequential_root_snapshots transport=unix_socket simulator=in_process profile=release"
    );
    println!(
        "roundtrip_us p50={} p95={} p99={} max={}",
        samples[999], samples[1899], samples[1979], samples[1999]
    );
    device.close().await;
    stop.cancel();
    server.await??;
    Ok(())
}
