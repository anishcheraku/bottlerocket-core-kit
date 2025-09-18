//! Child process management for dbus-broker.
//!
//! This module provides the [`BrokerChild`] which manages the lifecycle of the spawned
//! dbus-broker process, including signal handling and resource cleanup.

use crate::error::{self, Result};
use snafu::ResultExt;
use std::os::unix::io::AsRawFd;
use std::os::unix::net::UnixDatagram;
use std::process::Stdio;
use tokio::net::UnixStream;
use tokio::process::{Child, Command};
use tokio::signal;

/// Hardcoded path to machine-id file in Bottlerocket
const MACHINE_ID_PATH: &str = "/etc/machine-id";

/// Default limits from dbus-launcher
/// See: https://github.com/bus1/dbus-broker/blob/b0db0890d1254477cf832e5f9f0a798360c80fd9/src/launch/launcher.c#L35
const DEFAULT_MAX_OUTGOING_BYTES: u64 = 8 * 1024 * 1024;
const DEFAULT_MAX_OUTGOING_UNIX_FDS: u64 = 64;
const DEFAULT_MAX_CONNECTIONS_PER_USER: u64 = 64;
const DEFAULT_MAX_MATCH_RULES_PER_CONNECTION: u64 = 256;

/// Calculated limits (per-user = per-connection × max-connections)
/// See: https://github.com/bus1/dbus-broker/blob/b0db0890d1254477cf832e5f9f0a798360c80fd9/src/launch/launcher.c#L1102-L1105
const MAX_BYTES: u64 = DEFAULT_MAX_CONNECTIONS_PER_USER * DEFAULT_MAX_OUTGOING_BYTES;
const MAX_FDS: u64 = DEFAULT_MAX_CONNECTIONS_PER_USER * DEFAULT_MAX_OUTGOING_UNIX_FDS;
const MAX_MATCHES: u64 = DEFAULT_MAX_CONNECTIONS_PER_USER * DEFAULT_MAX_MATCH_RULES_PER_CONNECTION;

#[derive(Debug)]
pub(crate) struct BrokerChildInitial {
    broker_path: String,
}

#[derive(Debug)]
pub(crate) struct BrokerChildRunning {
    child: Child,
    _journal_socket: UnixDatagram,
    _broker_socket: UnixStream,
}

impl BrokerChildInitial {
    /// Returns a new instance of the `BrokerChild`
    pub(crate) fn new(broker_path: &str) -> Result<Self> {
        Ok(Self {
            broker_path: broker_path.to_string(),
        })
    }

    /// Forks the dbus-broker with the parameters configured for the child
    pub(crate) fn run(
        self,
        journal_socket: UnixDatagram,
        broker_socket: UnixStream,
    ) -> Result<BrokerChildRunning> {
        debug!("Reading machine ID from {MACHINE_ID_PATH}");
        let machine_id = std::fs::read_to_string(MACHINE_ID_PATH)
            .context(error::MachineIdSnafu {
                path: MACHINE_ID_PATH,
            })?
            .trim()
            .to_string();
        debug!("Machine ID: {machine_id}");

        let mut cmd = Command::new(&self.broker_path);
        cmd.arg("--log")
            .arg(format!("{}", journal_socket.as_raw_fd()))
            .arg("--controller")
            .arg(format!("{}", broker_socket.as_raw_fd()))
            .arg("--machine-id")
            .arg(machine_id)
            .arg("--max-bytes")
            .arg(format!("{MAX_BYTES}"))
            .arg("--max-fds")
            .arg(format!("{MAX_FDS}"))
            .arg("--max-matches")
            .arg(format!("{MAX_MATCHES}"))
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());

        debug!(
            "Spawning dbus-broker with limits: max-bytes={MAX_BYTES}, max-fds={MAX_FDS}, max-matches={MAX_MATCHES}",
        );

        // Hold the reference to the spawned process, so that it can be killed if needed
        let child = cmd.spawn().context(error::BrokerSpawnSnafu)?;
        info!("dbus-broker spawned with PID: {:?}", child.id());

        // Pass down the sockets to the active running child, to prevent them from going out of
        // scope before the child exits
        Ok(BrokerChildRunning {
            child,
            _journal_socket: journal_socket,
            _broker_socket: broker_socket,
        })
    }
}

impl BrokerChildRunning {
    /// Wait for either the spawned process to complete, or for the SIGTERM signal to be sent
    pub(crate) async fn wait(mut self) -> Result<()> {
        debug!("Setting up signal handlers");
        let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate())
            .context(error::CreateSigTermSignalSnafu)?;
        // Wait for either the child process to stop, or for the kill signal
        tokio::select! {
            // Child process died naturally
            status = self.child.wait() => {
                let status = status.context(error::BrokerWaitSnafu)?;
                if status.success() {
                    info!("dbus-broker exited successfully: {status}");
                } else {
                    warn!("dbus-broker exited with status: {status}");
                }
                Ok(())
            }
            _ = sigterm.recv() => {
                info!("Received SIGTERM, shutting down");
                debug!("Killing dbus-broker process");
                let _ = self.child.kill().await;
                info!("Shutdown complete");
                Ok(())
            }
        }
    }
}
