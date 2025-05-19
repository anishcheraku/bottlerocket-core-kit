/// Potential errors during `ghostdog` execution.
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
    #[snafu(display("Failed to check if EFA device is attached: {}", source))]
    CheckEfaFailure { source: pciclient::PciClientError },
    #[snafu(display("Failed to check if Neuron device is attached: {}", source))]
    CheckNeuronFailure { source: pciclient::PciClientError },
    #[snafu(display("Did not detect EFA"))]
    NoEfaPresent,
    #[snafu(display("Did not detect Neuron"))]
    NoNeuronPresent,
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
