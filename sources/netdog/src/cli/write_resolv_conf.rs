use super::{error, primary_interface_name, Result};
use crate::cli::{fetch_net_config, Command};
use crate::dns::DnsSettings;
use crate::networkd::config::NETWORKD_CONFIG_DIR;
use argh::FromArgs;
use snafu::{ensure, ResultExt};
use std::{fs, path::Path};
use systemd_derive::{SystemdUnit, SystemdUnitSection};

static SYSTEMCTL: &str = "/usr/bin/systemctl";

#[derive(FromArgs, PartialEq, Debug)]
#[argh(subcommand, name = "write-resolv-conf")]
/// Writes /etc/resolv.conf, using DNS API settings if they exist
pub(crate) struct WriteResolvConfArgs {}

/// A struct representing an interface drop-in that overrides DNS-via-DHCP settings
#[derive(Debug, SystemdUnit)]
struct InterfaceDNSDropIn {
    network: Option<NetworkSection>,
    dhcp4: Option<Dhcp4Section>,
    dhcp6: Option<Dhcp6Section>,
    ipv6_accept_ra: Option<IPv6AcceptRASection>,
}

#[derive(Debug, SystemdUnitSection)]
#[systemd(section = "Network")]
struct NetworkSection {
    #[systemd(entry = "DNSDefaultRoute")]
    dns_default_route: Option<bool>,
}

#[derive(Debug, SystemdUnitSection)]
#[systemd(section = "DHCPv4")]
struct Dhcp4Section {
    #[systemd(entry = "UseDNS")]
    use_dns: Option<bool>,
    #[systemd(entry = "UseDomains")]
    use_domains: Option<bool>,
}

#[derive(Debug, SystemdUnitSection)]
#[systemd(section = "DHCPv6")]
struct Dhcp6Section {
    #[systemd(entry = "UseDNS")]
    use_dns: Option<bool>,
    #[systemd(entry = "UseDomains")]
    use_domains: Option<bool>,
}

#[derive(Debug, SystemdUnitSection)]
#[systemd(section = "IPv6AcceptRA")]
struct IPv6AcceptRASection {
    #[systemd(entry = "UseDNS")]
    use_dns: Option<bool>,
    #[systemd(entry = "UseDomains")]
    use_domains: Option<bool>,
}

impl InterfaceDNSDropIn {
    /// Given API DNS settings create an appropriate drop-in for a network interface.
    fn new(settings: &DnsSettings, is_primary: bool) -> Self {
        // Default to not using DNS values from DHCP, and do not use the interface's DNS route to
        // resolve domains not matching other config.  If the interface is the primary interface,
        // use DNS API settings to direct the appropriate interface configuration. This ensures
        // DNS configuration is sourced from settings and the primary interface only.
        let mut should_use_dns_from_dhcp = Some(false);
        let mut should_use_domains_from_dhcp = Some(false);
        let mut should_be_dns_default_route = Some(false);

        if is_primary {
            should_use_dns_from_dhcp = Some(!settings.has_name_servers());
            should_use_domains_from_dhcp = Some(!settings.has_search_domains());
            should_be_dns_default_route = should_use_dns_from_dhcp;
        }

        Self {
            network: Some(NetworkSection {
                dns_default_route: should_be_dns_default_route,
            }),
            dhcp4: Some(Dhcp4Section {
                use_dns: should_use_dns_from_dhcp,
                use_domains: should_use_domains_from_dhcp,
            }),
            dhcp6: Some(Dhcp6Section {
                use_dns: should_use_dns_from_dhcp,
                use_domains: should_use_domains_from_dhcp,
            }),
            ipv6_accept_ra: Some(IPv6AcceptRASection {
                use_dns: should_use_dns_from_dhcp,
                use_domains: should_use_domains_from_dhcp,
            }),
        }
    }
}

// If we have DNS name servers or search domain settings from the API, we want to ignore the
// corresponding values in the DHCP lease.  If we don't, then we want to use the values from DHCP.
// Toggle this functionality via a networkd interface drop-in.  Also write the global settings from
// the API as a systemd-resolved drop-in.
fn handle_dns_settings(primary_interface: String) -> Result<()> {
    let dns_settings = DnsSettings::from_config().context(error::GetDnsSettingsSnafu)?;

    // For each configured interface, create the drop-in directory and file
    let (maybe_net_config, _) = fetch_net_config()?;
    if let Some(net_config) = maybe_net_config {
        for interface in net_config.interfaces() {
            let interface_drop_in =
                InterfaceDNSDropIn::new(&dns_settings, interface.to_string() == primary_interface);

            // Remove the colons since the ID might be a MAC address
            let name = interface.to_string().replace(':', "");
            let dropin_dir_name = format!("10-{name}.network.d");
            let dropin_dir_path = Path::new(NETWORKD_CONFIG_DIR).join(&dropin_dir_name);
            fs::create_dir_all(&dropin_dir_path).context(error::DropInDirCreateSnafu {
                path: &dropin_dir_path,
            })?;

            let mut dropin_file_path = dropin_dir_path.join("10-dns");
            dropin_file_path.set_extension("conf");
            fs::write(&dropin_file_path, interface_drop_in.to_string()).context(
                error::DropInFileWriteSnafu {
                    path: dropin_file_path,
                },
            )?;
        }
    }

    // Write the systemd-resolved drop-in which will contain the DNS settings from the API
    dns_settings
        .write_resolved_dropin()
        .context(error::ResolvConfWriteFailedSnafu)?;

    // After all the above file writes have completed successfully, restart systemd-networkd and
    // systemd-resolved.
    let restart_networkd = Command::new(SYSTEMCTL)
        .args(["try-reload-or-restart", "--no-block", "systemd-networkd"])
        .output()
        .context(error::SystemctlExecutionSnafu)?;
    ensure!(
        restart_networkd.status.success(),
        error::FailedSystemctlSnafu {
            stderr: String::from_utf8_lossy(&restart_networkd.stderr)
        }
    );
    let restart_resolved = Command::new(SYSTEMCTL)
        .args(["try-reload-or-restart", "--no-block", "systemd-resolved"])
        .output()
        .context(error::SystemctlExecutionSnafu)?;
    ensure!(
        restart_resolved.status.success(),
        error::FailedSystemctlSnafu {
            stderr: String::from_utf8_lossy(&restart_resolved.stderr)
        }
    );

    Ok(())
}

pub(crate) fn run() -> Result<()> {
    let primary_interface = primary_interface_name()?;
    handle_dns_settings(primary_interface)?;

    Ok(())
}
