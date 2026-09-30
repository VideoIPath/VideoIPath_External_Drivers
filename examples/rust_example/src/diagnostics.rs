//! Local, bounded health checks never connect to VideoIPath or issue a device command.
use std::{
    path::Path,
    time::{Duration, SystemTime},
};

pub fn heartbeat(directory: &Path) -> std::io::Result<()> {
    // Rename publishes one complete observation; payloads and secrets never enter this file.
    std::fs::write(directory.join("heartbeat.next"), b"supervisor running\n")?;
    std::fs::rename(
        directory.join("heartbeat.next"),
        directory.join("heartbeat"),
    )
}

pub fn healthy(directory: &Path) -> bool {
    std::fs::metadata(directory.join("heartbeat"))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_some_and(|age| age < Duration::from_secs(15))
}

/// Expose only recognized booleans, not the raw contents of a mutable status file.
pub fn status(directory: &Path) -> std::io::Result<String> {
    use std::io::Read;
    let mut text = String::new();
    std::fs::File::open(directory.join("readiness"))?
        .take(128)
        .read_to_string(&mut text)?;
    let device = text.lines().any(|line| line == "device=true");
    let vip = text.lines().any(|line| line == "videoipath=true");
    Ok(format!("device={device}\nvideoipath={vip}"))
}

pub fn readiness(directory: &Path, device: bool, vip: bool) -> std::io::Result<()> {
    std::fs::write(
        directory.join("readiness.next"),
        format!("device={device}\nvideoipath={vip}\n"),
    )?;
    std::fs::rename(
        directory.join("readiness.next"),
        directory.join("readiness"),
    )
}
