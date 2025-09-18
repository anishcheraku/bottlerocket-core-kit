//! D-Bus broker management and lifecycle control.
//!
//! This module provides the [`BrokerManager`] which handles spawning and communicating
//! with the dbus-broker process, including policy transfer and systemd integration.

use crate::child::BrokerChildInitial;
use crate::dbus_policy::DbusPolicy;
use crate::error::{self, Result};
use crate::socket;
use snafu::ResultExt;
use std::mem;
use std::os::unix::io::BorrowedFd;
use tokio::net::UnixStream;
use zbus::conn::Builder;
use zbus::zvariant::{Fd, ObjectPath, Value as ZVariantValue};
use zbus::{proxy, Connection};

/// A proxy client used to interact with the spawned dbus broker once it establishes a connection
/// back to whippet
#[proxy(
    interface = "org.bus1.DBus.Broker",
    default_path = "/org/bus1/DBus/Broker"
)]
trait DBusBroker {
    /// Client call of the AddListener method to transfer the systemd socket and dbus policy to the
    /// broker
    fn add_listener(
        &self,
        path: ObjectPath<'_>,
        fd: Fd<'_>,
        policy: ZVariantValue<'_>,
    ) -> zbus::Result<()>;
}

/// Manages the lifecycle of the spawned broker
pub(crate) struct BrokerManager {
    policy: DbusPolicy,
    launcher_socket: UnixStream,
    broker_socket: UnixStream,
    child: BrokerChildInitial,
}

impl BrokerManager {
    pub(crate) fn new(broker_path: &str, policy: DbusPolicy) -> Result<Self> {
        debug!("Creating BrokerManager with broker path: {broker_path}");

        let (launcher_socket, broker_socket) = socket::controller_pair()?;
        let child = BrokerChildInitial::new(broker_path)?;

        Ok(Self {
            policy,
            child,
            broker_socket,
            launcher_socket,
        })
    }

    /// Spawn the broker and transfer the systemd socket and policy
    pub(crate) async fn run(self) -> Result<()> {
        let journal_socket = socket::journal_socket()?;
        // The broker has to be started on the background before the connection is created
        // otherwise the creation of the connection hangs since there isn't anything connected on
        // the other side of the pair.
        let running = self.child.run(journal_socket, self.broker_socket)?;

        // Configure the broker in the background to prevent connections from hanging
        // if the child dies before it gets to use the inherited socket (e.g. bad flag used)
        let configure_task = tokio::spawn(configure_broker(self.policy, self.launcher_socket));

        // Wait for the two tasks to complete, otherwise we can't extract the error from either
        // thread. If any error occurs in the configure task, the connection socket will be dropped
        // and the broker task will exit
        let (configure_result, broker_result) = tokio::join! {
            configure_task,
            running.wait()
        };

        let configure_result = configure_result.context(error::DbusBrokerConfigureSnafu)?;
        configure_result?;
        broker_result?;

        Ok(())
    }
}

/// Configure the broker with the provided policy notifying systemd when ready
async fn configure_broker(policy: DbusPolicy, launcher_socket: UnixStream) -> Result<()> {
    debug!("Creating D-Bus connection to broker controller");
    let connection_builder: Builder<'_> = Builder::unix_stream(launcher_socket).p2p();
    let connection = connection_builder
        .build()
        .await
        .context(error::DbusBrokerConnectionBuildSnafu)?;

    debug!("Adding listener with parsed policy");
    set_listener_policy(policy, &connection).await?;
    info!("Policy applied successfully");
    notify_systemd_ready()?;
    info!("Notified systemd that service is ready");
    // The connection has to live for as long as whippet runs, otherwise the broker exits. Forget
    // the connection to prevent dropping it
    mem::forget(connection);

    Ok(())
}

/// Call AddListener method with policy
async fn set_listener_policy(policy: DbusPolicy, connection: &Connection) -> Result<()> {
    debug!("Starting AddListener call");

    debug!("Getting systemd listener socket");
    let listener_fd = socket::listener_socket()?;
    info!("Using listener socket FD: {listener_fd}");

    let object_path =
        ObjectPath::try_from("/org/bus1/DBus/Listener/0").context(error::ParseObjectPathSnafu)?;
    // The listener file descriptor technically isn't owned by whippet and it is managed by systemd.
    // Wrap the file descriptor around a BorrowedFd to prevent the file descriptor from being
    // closed once the broker goes out of scope and let systemd handle the lifecycle of the socket.
    let fd_handle = Fd::from(unsafe { BorrowedFd::borrow_raw(listener_fd as i32) });

    info!("Setting broker listener socket and policy");

    let policy = ZVariantValue::from(policy);
    debug!("Creating D-Bus broker proxy");
    let proxy = DBusBrokerProxy::builder(connection)
        .destination("org.bus1.DBus.Broker")
        .context(error::DbusBrokerProxyBuildSnafu)?
        .build()
        .await
        .context(error::DbusBrokerProxyBuildSnafu)?;

    // This is the call that transfers the policy to the broker. The broker uses the Dbus format to
    // communicate with the launcher. This call will wait until the broker loads, parses and applies
    // the policy, returning an error if the policy was invalid.
    proxy
        .add_listener(object_path, fd_handle, policy)
        .await
        .context(error::DbusBrokerAddListenerSnafu)?;

    info!("Done setting broker listener socket and policy");
    Ok(())
}

/// Notify systemd that the service is ready
fn notify_systemd_ready() -> Result<()> {
    let socket = socket::notify_socket()?;

    socket
        .send(b"READY=1")
        .context(error::SystemdNotificationSnafu)?;

    debug!("Sent READY=1 notification to systemd");
    Ok(())
}
