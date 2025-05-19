use crate::error;
use crate::error::Result;
use snafu::{ensure, ResultExt};
use std::ffi::OsString;
use std::ops::Deref;
use std::path::Path;
use std::str::FromStr;
use std::{fs, str, vec};

const SYS_CLASS_DIR: &str = "/sys/class";
const INFINIBAND_CLASS_NAME: &str = "infiniband";
const SW_MNG_VALUE: &str = "SW_MNG";

/// Generic device struct found from sysfs
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) struct InfinibandDevice {
    pub(crate) name: OsString,
    pub(crate) ports: Vec<InfinibandPort>,
}

/// Finds all the devices in sysfs for a given class sorted by name
pub(crate) fn find_infiniband_devices() -> Result<Vec<InfinibandDevice>> {
    let devices_path = Path::new(SYS_CLASS_DIR).join(INFINIBAND_CLASS_NAME);
    let mut found_devices: Vec<InfinibandDevice> = vec![];
    if !devices_path.exists() {
        // Nothing to do since no devices detected
        return Ok(found_devices);
    }

    let sys_devices_dirs = fs::read_dir(devices_path).context(error::InfinibandSysDevicesSnafu)?;
    for device in sys_devices_dirs {
        let device = device.context(error::InfinibandDeviceSnafu)?;
        let device_name: OsString = device.file_name();
        found_devices.push(InfinibandDevice {
            name: device_name,
            ports: vec![],
        })
    }

    found_devices.sort();
    Ok(found_devices)
}

impl InfinibandDevice {
    /// Returns true if the SW_MNG_VALUE is found in the Vital Product Data file for given sysfs device
    pub(crate) fn is_device_sw_mng(&self) -> Result<bool> {
        let vpd_file = Path::new(SYS_CLASS_DIR)
            .join(INFINIBAND_CLASS_NAME)
            .join(self.name.as_os_str())
            .join("device")
            .join("vpd");
        if vpd_file.exists() {
            let vpd_file_bytes: &[u8] =
                &std::fs::read(vpd_file.as_path()).context(error::ReadFileSnafu {
                    path: vpd_file.as_path(),
                })?;
            let needle = SW_MNG_VALUE.as_bytes();
            if vpd_file_bytes
                .windows(needle.len())
                .any(|window| window == needle)
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    // Given a device in sysfs, iterate over the known paths where ports are defined
    // Iterate over the ports and read in the capability mask and Port GUID for each port
    pub(crate) fn find_ports_for_device(self) -> Result<Vec<InfinibandPort>> {
        let mut found_ports: Vec<InfinibandPort> = vec![];
        let ports_path = Path::new(SYS_CLASS_DIR)
            .join(INFINIBAND_CLASS_NAME)
            .join(self.name)
            .join("ports");

        let port_directories =
            fs::read_dir(ports_path).context(error::InfinibandSysDevicesSnafu)?;
        for port in port_directories {
            let port = port.context(error::InfinibandSysDevicesSnafu)?;
            let capability_mask_path = port.path().join("cap_mask");
            if !capability_mask_path.exists() {
                continue;
            }
            let capability_mask = std::fs::read_to_string(capability_mask_path.as_path()).context(
                error::ReadFileSnafu {
                    path: capability_mask_path.as_path(),
                },
            )?;
            let first_guid_path = port.path().join("gids").join("0");
            if !first_guid_path.exists() {
                continue;
            }
            let first_guid = std::fs::read_to_string(first_guid_path.as_path()).context(
                error::ReadFileSnafu {
                    path: first_guid_path.as_path(),
                },
            )?;

            // If a capability mask and port guid have been read, create the Infiniband Port
            found_ports.push(InfinibandPort {
                capability_mask: CapabilityMask::from_str(capability_mask.as_str())?,
                port_guid: PortGuid::from_str(first_guid.as_str())?,
            });
        }
        Ok(found_ports)
    }
}

/// Struct that holds the two important values for an Infiniband Port when checking for
/// Subnet Management capabilties.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) struct InfinibandPort {
    /// Capability Mask is read in as a string such as 0xa751e848
    pub(crate) capability_mask: CapabilityMask,
    /// Primary(first enumerated) PortGuid for a Port
    pub(crate) port_guid: PortGuid,
}

impl InfinibandPort {
    /// is_sm_enabled checks the specific capability bit for Subnet Management means its enabled
    // The actual bit means `isSMDisabled`, so check if its not zero
    pub(crate) fn is_sm_enabled(&self) -> bool {
        // SM is enabled if the eleventh bit ("SM disabled") is clear
        (*self.capability_mask & (1 << 10)) == 0
    }
}

/// Port GUID is a u64 that is read in as a string with : separting 8 sections but is written out in a form such as 0x
///
/// For example "fe80:0000:0000:0000:e09d:7303:003f:3bf8" will be stored and displayed as "0xe09d7303003f3bf8"
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) struct PortGuid(String);

impl FromStr for PortGuid {
    type Err = error::Error;
    /// String is expected to look like "fe80:0000:0000:0000:e09d:7303:003f:3bf8"
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let potential_guid = s.trim().to_string();
        ensure!(
            potential_guid.starts_with("fe80"),
            error::InvalidPortGuidStringSnafu {
                guid: potential_guid
            }
        );
        let parts: Vec<&str> = potential_guid.split(":").collect();
        ensure!(
            parts.len() == 8,
            error::InvalidPortGuidStringSnafu {
                guid: potential_guid
            }
        );
        for part in parts.iter() {
            ensure!(
                part.len() == 4,
                error::InvalidPortGuidStringSnafu {
                    guid: potential_guid
                }
            );
        }

        Ok(PortGuid(format!(
            "0x{}{}{}{}",
            parts[4], parts[5], parts[6], parts[7]
        )))
    }
}

impl Deref for PortGuid {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Capability Mask struct to help with FromStr and bit masking
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) struct CapabilityMask(u32);

impl FromStr for CapabilityMask {
    type Err = error::Error;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let mask = hex_string_to_u32(s.trim()).context(error::CapabilityCheckSnafu { mask: s })?;
        Ok(CapabilityMask(mask))
    }
}

impl Deref for CapabilityMask {
    type Target = u32;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

// Helper function to take the string Capability mask and convert to u32
fn hex_string_to_u32(hex: &str) -> std::result::Result<u32, std::num::ParseIntError> {
    let hex_str = hex.strip_prefix("0x").unwrap_or(hex);
    u32::from_str_radix(hex_str, 16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hex_to_u32_valid_hex_with_prefix() {
        assert_eq!(hex_string_to_u32("0xa750e84a").unwrap(), 0xa750e84a);
        assert_eq!(hex_string_to_u32("0xffff").unwrap(), 0xffff);
        assert_eq!(hex_string_to_u32("abcdef12").unwrap(), 0xABCDEF12);
        assert_eq!(hex_string_to_u32("0x0").unwrap(), 0x0);
    }

    #[test]
    fn test_hex_to_u32_valid_hex_without_prefix() {
        assert_eq!(hex_string_to_u32("a750e84a").unwrap(), 0xa750e84a);
        assert_eq!(hex_string_to_u32("ffff").unwrap(), 0xffff);
        assert_eq!(hex_string_to_u32("0").unwrap(), 0x0);
    }

    #[test]
    fn test_hex_to_u32_invalid_hex() {
        assert!(hex_string_to_u32("invalid").is_err());
        assert!(hex_string_to_u32("0xgggg").is_err());
        assert!(hex_string_to_u32("").is_err());
    }

    #[test]
    fn test_hex_to_u32_overflow() {
        // Value larger than u32::MAX
        assert!(hex_string_to_u32("0xffffffffff").is_err());
    }

    #[test]
    fn test_valid_guid_creation() {
        let valid_guid = "fe80:0000:0000:0000:e09d:7303:003f:3bf8";
        let result = PortGuid::from_str(valid_guid);
        assert!(result.is_ok());
        assert_eq!(result.as_ref().unwrap().0, "0xe09d7303003f3bf8");
        assert_eq!(*result.unwrap(), "0xe09d7303003f3bf8");
    }

    #[test]
    fn test_valid_guid_with_whitespace() {
        let valid_guid = "  fe80:0000:0000:0000:e09d:7303:003f:3bf8  ";
        let result = PortGuid::from_str(valid_guid);
        assert!(result.is_ok());
        assert_eq!(*result.unwrap(), "0xe09d7303003f3bf8");
    }

    #[test]
    fn test_invalid_guid_prefix() {
        let invalid_guids = [
            "abcd:0000:0000:0000:e09d:7303:003f:3bf8",
            "fe81:0000:0000:0000:e09d:7303:003f:3bf8",
            "",
            "fe",
            "fe8",
        ];

        for invalid_guid in invalid_guids.iter() {
            let result = PortGuid::from_str(invalid_guid);
            assert!(result.is_err());

            match result {
                Err(e) => {
                    assert!(matches!(e, error::Error::InvalidPortGuidString { .. }));
                }
                Ok(_) => panic!("Expected error for invalid GUID"),
            }
        }
    }

    #[test]
    fn test_guid_clone() {
        let guid = PortGuid::from_str("fe80:0000:0000:0000:e09d:7303:003f:3bf8").unwrap();
        let cloned_guid = guid.clone();
        assert_eq!(guid.0, cloned_guid.0);
    }

    #[test]
    fn test_guid_debug() {
        let guid = PortGuid::from_str("fe80:0000:0000:0000:e09d:7303:003f:3bf8").unwrap();
        let debug_output = format!("{:?}", guid);
        assert!(debug_output.contains("0xe09d7303003f3bf8"));
    }

    #[test]
    fn test_is_sm_enabled() {
        let test_cases = vec![
            // SM enabled cases (bit 10 is 0)
            ("0x00000000", true), // All bits 0
            ("0xa750e84a", true), // Expected mask that passes
            ("0xfffffbff", true), // Only bit 10 is 0
            ("0x00000001", true), // Only first bit set
            // SM disabled cases (bit 10 is 1)
            ("0x00000400", false), // Only bit 10 set
            ("0xffffffff", false), // All bits set
            ("0x00000401", false), // Bit 10 and first bit set
        ];

        for (capability_mask, expected) in test_cases {
            let port = InfinibandPort {
                capability_mask: CapabilityMask::from_str(capability_mask).unwrap(),
                port_guid: PortGuid::from_str("fe80:0000:0000:0000:e09d:7303:003f:3bf8").unwrap(),
            };

            assert_eq!(
                port.is_sm_enabled(),
                expected,
                "Failed for capability_mask: {}",
                capability_mask
            );
        }
    }
}
