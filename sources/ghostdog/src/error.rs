/// Potential errors during `ghostdog` execution.
use std::{io, path::PathBuf};

use snafu::Snafu;
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(super)))]
pub(super) enum Error {
    #[snafu(display("Failed to open '{}': {}", path.display(), source))]
    DeviceOpen {
        path: std::path::PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("Failed to execute NVMe command for device '{}': {}", path.display(), source))]
    NvmeCommand {
        path: std::path::PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("Invalid device info for device '{}'", path.display()))]
    InvalidDeviceInfo { path: std::path::PathBuf },
    #[snafu(display("Unable to read infiniband devices from sysfs: {}", source))]
    InfinibandSysDevices { source: std::io::Error },
    #[snafu(display("Unable to read device file from sysfs: {}", source))]
    InfinibandDevice { source: std::io::Error },
    #[snafu(display("Could not get hex value from '{}': {}", mask, source))]
    CapabilityCheck {
        mask: String,
        source: std::num::ParseIntError,
    },
    #[snafu(display("Found invalid GUID '{}'", guid))]
    InvalidPortGuidString { guid: String },
    #[snafu(display("Failed to check if EFA device is attached: {}", source))]
    CheckEfaFailure { source: pciclient::PciClientError },
    #[snafu(display("Failed to check if Neuron device is attached: {}", source))]
    CheckNeuronFailure { source: pciclient::PciClientError },
    #[snafu(display("Did not detect EFA"))]
    NoEfaPresent,
    #[snafu(display("Did not detect Neuron"))]
    NoNeuronPresent,
    #[snafu(display("'{}' has no parent directory", path.display()))]
    NoParentDirectory { path: std::path::PathBuf },
    #[snafu(display("Failed to open '{}': {}", path.display(), source))]
    OpenFile {
        path: std::path::PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("Failed to read '{}': {}", path.display(), source))]
    ReadFile {
        path: std::path::PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("Failed to create temporary file in {}: {}", path.display(), source))]
    CreateTempFile { path: PathBuf, source: io::Error },
    #[snafu(display("Failed to write temporary file: {}", source))]
    WriteTempFile { source: io::Error },
    #[snafu(display("Failed to move temporary file to {}: {}", path.display(), source))]
    PersistTempFile {
        path: PathBuf,
        source: tempfile::PersistError,
    },
    #[snafu(display("Couldn't parse the GPU Devices File: {}", source))]
    ParseGpuDevicesFile { source: serde_json::Error },
    #[snafu(display("Failed to list PCI devices: {}", source))]
    ListPciDevices { source: pciclient::PciClientError },
    #[snafu(display("{} is not preferred driver: {}", requested, preferred))]
    DriverMismatch {
        requested: String,
        preferred: String,
    },
}

pub(crate) type Result<T> = std::result::Result<T, Error>;
