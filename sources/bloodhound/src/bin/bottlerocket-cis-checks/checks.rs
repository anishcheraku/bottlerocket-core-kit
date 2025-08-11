use bloodhound::results::{CheckStatus, Checker, CheckerMetadata, CheckerResult, Mode};
use bloodhound::*;
use std::os::unix::fs::PermissionsExt;
use system_access::SystemAccess;
use walkdir::WalkDir;

const PROC_MODULES_FILE: &str = "/proc/modules";
const PROC_CMDLINE_FILE: &str = "/proc/cmdline";
const LOCKDOWN_FILE: &str = "/sys/kernel/security/lockdown";
const CHRONY_CONF_FILE: &str = "/etc/chrony.conf";
const JOURNALD_CONF_FILE: &str = "/usr/lib/systemd/journald.conf.d/journald.conf";
const SYSCTL_CMD: &str = "/usr/sbin/sysctl";
const SYSTEMCTL_CMD: &str = "/usr/bin/systemctl";
const MODPROBE_CMD: &str = "/bin/modprobe";
const SESTATUS_CMD: &str = "/usr/bin/sestatus";
const IPTABLES_CMD: &str = "/usr/sbin/iptables";
const IP6TABLES_CMD: &str = "/usr/sbin/ip6tables";

// =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<=

pub struct BR01010101Checker {}

impl Checker for BR01010101Checker {
    fn execute(&self, sac: &dyn SystemAccess) -> CheckerResult {
        let mut module_result = CheckerResult::default();

        // Make sure UDF isn't already loaded
        if let Ok(found) = look_for_word_in_file(sac, PROC_MODULES_FILE, "udf") {
            if found {
                module_result.error = "udf is currently loaded".to_string();
                module_result.status = CheckStatus::FAIL;
                return module_result;
            }
        } else {
            module_result.error =
                "unable to parse modprobe output to check if udf is enabled".to_string();
            return module_result;
        }

        // Make sure the ability to load UDF is disabled
        check_output_contains!(
            sac,
            MODPROBE_CMD,
            &["-n", "-v", "udf"],
            &["install /bin/true"],
            "unable to parse modprobe output to check if udf is enabled",
            "modprobe for udf is not disabled"
        )
    }

    fn metadata(&self) -> CheckerMetadata {
        CheckerMetadata {
            title: "Ensure mounting of udf filesystems is disabled".to_string(),
            id: "1.1.1.1".to_string(),
            level: 2,
            name: "br01010101".to_string(),
            mode: Mode::Automatic,
        }
    }
}

// =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<=

pub struct BR01030100Checker {}

impl Checker for BR01030100Checker {
    fn execute(&self, sac: &dyn SystemAccess) -> CheckerResult {
        check_file_contains!(
            sac,
            PROC_CMDLINE_FILE,
            &[
                "dm-mod.create=root,,,ro,0",
                "root=/dev/dm-0",
                "restart_on_corruption",
            ],
            "unable to verify cmdline includes dm-verity settings",
            "unable to verify dm-verity enforcement, settings not found"
        )
    }

    fn metadata(&self) -> CheckerMetadata {
        CheckerMetadata {
            title: "Ensure dm-verity is configured".to_string(),
            id: "1.3.1".to_string(),
            level: 1,
            name: "br01030100".to_string(),
            mode: Mode::Automatic,
        }
    }
}

// =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<=

pub struct BR01040100Checker {}

impl Checker for BR01040100Checker {
    fn execute(&self, sac: &dyn SystemAccess) -> CheckerResult {
        check_output_contains!(
            sac,
            SYSCTL_CMD,
            &["fs.suid_dumpable"],
            &["fs.suid_dumpable = 0"],
            "unable to verify fs.suid_dumpable setting",
            "setuid core dumps are not disabled"
        )
    }

    fn metadata(&self) -> CheckerMetadata {
        CheckerMetadata {
            title: "Ensure setuid programs do not create core dumps".to_string(),
            id: "1.4.1".to_string(),
            level: 1,
            name: "br01040100".to_string(),
            mode: Mode::Automatic,
        }
    }
}

// =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<=

pub struct BR01040200Checker {}

impl Checker for BR01040200Checker {
    fn execute(&self, sac: &dyn SystemAccess) -> CheckerResult {
        check_output_contains!(
            sac,
            SYSCTL_CMD,
            &["kernel.randomize_va_space"],
            &["kernel.randomize_va_space = 2"],
            "unable to verify kernel.randomize_va_space setting",
            "Address space layout randomization is not enabled"
        )
    }

    fn metadata(&self) -> CheckerMetadata {
        CheckerMetadata {
            title: "Ensure address space layout randomization (ASLR) is enabled".to_string(),
            id: "1.4.2".to_string(),
            level: 1,
            name: "br01040200".to_string(),
            mode: Mode::Automatic,
        }
    }
}

// =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<=

pub struct BR01040300Checker {}

impl Checker for BR01040300Checker {
    fn execute(&self, sac: &dyn SystemAccess) -> CheckerResult {
        check_output_contains!(
            sac,
            SYSCTL_CMD,
            &["kernel.unprivileged_bpf_disabled"],
            &["kernel.unprivileged_bpf_disabled = 1"],
            "unable to verify kernel.unprivileged_bpf_disabled setting",
            "unprivileged eBPF is not disabled"
        )
    }

    fn metadata(&self) -> CheckerMetadata {
        CheckerMetadata {
            title: "Ensure unprivileged eBPF is disabled".to_string(),
            id: "1.4.3".to_string(),
            level: 1,
            name: "br01040300".to_string(),
            mode: Mode::Automatic,
        }
    }
}

// =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<=

pub struct BR01040400Checker {}

impl Checker for BR01040400Checker {
    fn execute(&self, sac: &dyn SystemAccess) -> CheckerResult {
        check_output_contains!(
            sac,
            SYSCTL_CMD,
            &["user.max_user_namespaces"],
            &["user.max_user_namespaces = 0"],
            "unable to verify user.max_user_namespaces setting",
            "user namespaces are not disabled"
        )
    }

    fn metadata(&self) -> CheckerMetadata {
        CheckerMetadata {
            title: "Ensure user namespaces are disabled".to_string(),
            id: "1.4.4".to_string(),
            level: 2,
            name: "br01040400".to_string(),
            mode: Mode::Automatic,
        }
    }
}

// =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<=

pub struct BR01050100Checker {}

impl Checker for BR01050100Checker {
    fn execute(&self, sac: &dyn SystemAccess) -> CheckerResult {
        let mut result = CheckerResult::default();

        // Trying to avoid bringing in regex for now
        let to_match = &[
            ("SELinux status: ", " enabled"),
            ("Loaded policy name: ", " fortified"),
            ("Current mode: ", " enforcing"),
            ("Mode from config file: ", " enforcing"),
            ("Policy MLS status: ", " enabled"),
            ("Policy deny_unknown status: ", " denied"),
            ("Memory protection checking: ", " actual (secure)"),
        ];

        if let Ok(output) = sac.command_output(SESTATUS_CMD, &[]) {
            let mut matched = 0;

            if output.status.success() {
                let mp_output = String::from_utf8_lossy(&output.stdout).to_string();
                for line in mp_output.lines() {
                    for match_line in to_match {
                        if line.contains(match_line.0) && line.contains(match_line.1) {
                            matched += 1;
                            break;
                        }
                    }
                }

                if to_match.len() == matched {
                    result.status = CheckStatus::PASS;
                } else {
                    result.error = "Unable to find expected SELinux values".to_string();
                    result.status = CheckStatus::FAIL;
                }
            }
        } else {
            result.error = "unable to verify SELinux settings".to_string();
        }

        result
    }

    fn metadata(&self) -> CheckerMetadata {
        CheckerMetadata {
            title: "Ensure SELinux is configured".to_string(),
            id: "1.5.1".to_string(),
            level: 1,
            name: "br01050100".to_string(),
            mode: Mode::Automatic,
        }
    }
}

// =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<=

pub struct BR01050200Checker {}

impl Checker for BR01050200Checker {
    fn execute(&self, sac: &dyn SystemAccess) -> CheckerResult {
        check_file_contains!(
            sac,
            LOCKDOWN_FILE,
            &["[integrity]"],
            "unable to verify lockdown mode",
            "lockdown integrity mode is not enabled"
        )
    }

    fn metadata(&self) -> CheckerMetadata {
        CheckerMetadata {
            title: "Ensure Lockdown is configured".to_string(),
            id: "1.5.2".to_string(),
            level: 2,
            name: "br01050200".to_string(),
            mode: Mode::Automatic,
        }
    }
}

// =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<=

pub struct BR02010101Checker {}

impl Checker for BR02010101Checker {
    fn execute(&self, sac: &dyn SystemAccess) -> CheckerResult {
        let result = check_file_contains!(
            sac,
            CHRONY_CONF_FILE,
            &["pool"],
            "unable to verify time-servers setting",
            "no ntp servers are configured"
        );

        // Check if we need to continue
        if result.status == CheckStatus::FAIL {
            return result;
        }

        check_output_contains!(
            sac,
            SYSTEMCTL_CMD,
            &["show", "--property", "ActiveState", "chronyd"],
            &["ActiveState=active"],
            "unable to verify chronyd service enabled",
            "chronyd NTP service is not enabled"
        )
    }

    fn metadata(&self) -> CheckerMetadata {
        CheckerMetadata {
            title: "Ensure chrony is configured".to_string(),
            id: "2.1.1.1".to_string(),
            level: 1,
            name: "br02010101".to_string(),
            mode: Mode::Automatic,
        }
    }
}

// =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<=

pub struct BR03010100Checker {}

impl Checker for BR03010100Checker {
    fn execute(&self, sac: &dyn SystemAccess) -> CheckerResult {
        let settings = [
            "net.ipv4.conf.all.send_redirects",
            "net.ipv4.conf.default.send_redirects",
        ];

        let output = [
            "net.ipv4.conf.all.send_redirects = 0",
            "net.ipv4.conf.default.send_redirects = 0",
        ];

        check_output_contains!(
            sac,
            SYSCTL_CMD,
            &settings,
            &output,
            "unable to verify redirect settings",
            "redirects not disabled"
        )
    }

    fn metadata(&self) -> CheckerMetadata {
        CheckerMetadata {
            title: "Ensure packet redirect sending is disabled".to_string(),
            id: "3.1.1".to_string(),
            level: 2,
            name: "br03010100".to_string(),
            mode: Mode::Automatic,
        }
    }
}

// =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<=

pub struct BR03020100Checker {}

impl Checker for BR03020100Checker {
    fn execute(&self, sac: &dyn SystemAccess) -> CheckerResult {
        let settings = [
            "net.ipv4.conf.all.accept_source_route",
            "net.ipv4.conf.default.accept_source_route",
            "net.ipv6.conf.all.accept_source_route",
            "net.ipv6.conf.default.accept_source_route",
        ];

        let output = [
            "net.ipv4.conf.all.accept_source_route = 0",
            "net.ipv4.conf.default.accept_source_route = 0",
            "net.ipv6.conf.all.accept_source_route = 0",
            "net.ipv6.conf.default.accept_source_route = 0",
        ];

        check_output_contains!(
            sac,
            SYSCTL_CMD,
            &settings,
            &output,
            "unable to verify source route settings",
            "accept source route not disabled"
        )
    }

    fn metadata(&self) -> CheckerMetadata {
        CheckerMetadata {
            title: "Ensure source routed packets are not accepted".to_string(),
            id: "3.2.1".to_string(),
            level: 2,
            name: "br03020100".to_string(),
            mode: Mode::Automatic,
        }
    }
}

// =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<=

pub struct BR03020200Checker {}

impl Checker for BR03020200Checker {
    fn execute(&self, sac: &dyn SystemAccess) -> CheckerResult {
        let settings = [
            "net.ipv4.conf.all.accept_redirects",
            "net.ipv4.conf.default.accept_redirects",
            "net.ipv6.conf.all.accept_redirects",
            "net.ipv6.conf.default.accept_redirects",
        ];

        let output = [
            "net.ipv4.conf.all.accept_redirects = 0",
            "net.ipv4.conf.default.accept_redirects = 0",
            "net.ipv6.conf.all.accept_redirects = 0",
            "net.ipv6.conf.default.accept_redirects = 0",
        ];

        check_output_contains!(
            sac,
            SYSCTL_CMD,
            &settings,
            &output,
            "unable to verify redirect settings",
            "accept redirects not disabled"
        )
    }

    fn metadata(&self) -> CheckerMetadata {
        CheckerMetadata {
            title: "Ensure ICMP redirects are not accepted".to_string(),
            id: "3.2.2".to_string(),
            level: 2,
            name: "br03020200".to_string(),
            mode: Mode::Automatic,
        }
    }
}

// =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<=

pub struct BR03020300Checker {}

impl Checker for BR03020300Checker {
    fn execute(&self, sac: &dyn SystemAccess) -> CheckerResult {
        let settings = [
            "net.ipv4.conf.all.secure_redirects",
            "net.ipv4.conf.default.secure_redirects",
        ];

        let output = [
            "net.ipv4.conf.all.secure_redirects = 0",
            "net.ipv4.conf.default.secure_redirects = 0",
        ];

        check_output_contains!(
            sac,
            SYSCTL_CMD,
            &settings,
            &output,
            "unable to verify secure redirect settings",
            "secure redirects not disabled"
        )
    }

    fn metadata(&self) -> CheckerMetadata {
        CheckerMetadata {
            title: "Ensure secure ICMP redirects are not accepted".to_string(),
            id: "3.2.3".to_string(),
            level: 2,
            name: "br03020300".to_string(),
            mode: Mode::Automatic,
        }
    }
}

// =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<=

pub struct BR03020400Checker {}

impl Checker for BR03020400Checker {
    fn execute(&self, sac: &dyn SystemAccess) -> CheckerResult {
        let settings = [
            "net.ipv4.conf.all.log_martians",
            "net.ipv4.conf.default.log_martians",
        ];

        let output = [
            "net.ipv4.conf.all.log_martians = 1",
            "net.ipv4.conf.default.log_martians = 1",
        ];

        check_output_contains!(
            sac,
            SYSCTL_CMD,
            &settings,
            &output,
            "unable to verify martian packet logging settings",
            "martian packet logging not enabled"
        )
    }

    fn metadata(&self) -> CheckerMetadata {
        CheckerMetadata {
            title: "Ensure suspicious packets are logged".to_string(),
            id: "3.2.4".to_string(),
            level: 2,
            name: "br03020400".to_string(),
            mode: Mode::Automatic,
        }
    }
}

// =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<=

pub struct BR03020500Checker {}

impl Checker for BR03020500Checker {
    fn execute(&self, sac: &dyn SystemAccess) -> CheckerResult {
        check_output_contains!(
            sac,
            SYSCTL_CMD,
            &["net.ipv4.icmp_echo_ignore_broadcasts"],
            &["net.ipv4.icmp_echo_ignore_broadcasts = 1"],
            "unable to verify broadcast ICMP requests setting",
            "broadcast ICMP requests not ignored"
        )
    }

    fn metadata(&self) -> CheckerMetadata {
        CheckerMetadata {
            title: "Ensure broadcast ICMP requests are ignored".to_string(),
            id: "3.2.5".to_string(),
            level: 1,
            name: "br03020500".to_string(),
            mode: Mode::Automatic,
        }
    }
}

// =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<=

pub struct BR03020600Checker {}

impl Checker for BR03020600Checker {
    fn execute(&self, sac: &dyn SystemAccess) -> CheckerResult {
        check_output_contains!(
            sac,
            SYSCTL_CMD,
            &["net.ipv4.icmp_ignore_bogus_error_responses"],
            &["net.ipv4.icmp_ignore_bogus_error_responses = 1"],
            "unable to verify bogus ICMP bogus requests setting",
            "ignore bogus ICMP requests not ignored"
        )
    }

    fn metadata(&self) -> CheckerMetadata {
        CheckerMetadata {
            title: "Ensure bogus ICMP responses are ignored".to_string(),
            id: "3.2.6".to_string(),
            level: 1,
            name: "br03020600".to_string(),
            mode: Mode::Automatic,
        }
    }
}

// =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<=

pub struct BR03020700Checker {}

impl Checker for BR03020700Checker {
    fn execute(&self, sac: &dyn SystemAccess) -> CheckerResult {
        check_output_contains!(
            sac,
            SYSCTL_CMD,
            &["net.ipv4.tcp_syncookies"],
            &["net.ipv4.tcp_syncookies = 1"],
            "unable to verify SYN flood cookie protection setting",
            "SYN flood cookie protection not enabled"
        )
    }

    fn metadata(&self) -> CheckerMetadata {
        CheckerMetadata {
            title: "Ensure TCP SYN Cookies is enabled".to_string(),
            id: "3.2.7".to_string(),
            level: 1,
            name: "br03020700".to_string(),
            mode: Mode::Automatic,
        }
    }
}

// =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<=

pub struct BR03030100Checker {}

impl Checker for BR03030100Checker {
    fn execute(&self, sac: &dyn SystemAccess) -> CheckerResult {
        let mut result = CheckerResult::default();

        // Make sure sctp isn't already loaded
        if let Ok(found) = look_for_word_in_file(sac, PROC_MODULES_FILE, "sctp") {
            if found {
                result.error = "sctp is currently loaded".to_string();
                result.status = CheckStatus::FAIL;
                return result;
            }
        } else {
            result.error = "unable to parse modules to check for sctp".to_string();
            return result;
        }

        check_output_contains!(
            sac,
            MODPROBE_CMD,
            &["-n", "-v", "sctp"],
            &["install /bin/true"],
            "unable to parse modprobe output to check if sctp is enabled",
            "modprobe for sctp is not disabled"
        )
    }

    fn metadata(&self) -> CheckerMetadata {
        CheckerMetadata {
            title: "Ensure SCTP is disabled".to_string(),
            id: "3.3.1".to_string(),
            level: 2,
            name: "br03030100".to_string(),
            mode: Mode::Automatic,
        }
    }
}

// =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<=

pub struct BR03040101Checker {}

impl Checker for BR03040101Checker {
    fn execute(&self, sac: &dyn SystemAccess) -> CheckerResult {
        let output = &[
            "Chain INPUT (policy DROP)",
            "Chain FORWARD (policy DROP)",
            "Chain OUTPUT (policy DROP)",
        ];

        check_output_contains!(
            sac,
            IPTABLES_CMD,
            &["-L"],
            output,
            "unable to verify iptables settings",
            "unable to find expected iptables values"
        )
    }

    fn metadata(&self) -> CheckerMetadata {
        CheckerMetadata {
            title: "Ensure IPv4 default deny firewall policy".to_string(),
            id: "3.4.1.1".to_string(),
            level: 2,
            name: "br03040101".to_string(),
            mode: Mode::Automatic,
        }
    }
}

// =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<=

pub struct BR03040102Checker {}

impl Checker for BR03040102Checker {
    fn execute(&self, sac: &dyn SystemAccess) -> CheckerResult {
        let mut result = CheckerResult::default();

        // Order matters here, so need to find the first one, then look for the second one
        let first = (
            "ACCEPT",
            "--  lo     *       0.0.0.0/0            0.0.0.0/0",
        );
        let second = ("DROP", "--  *      *       127.0.0.0/8          0.0.0.0/0");

        if let Ok(output) = sac.command_output(IPTABLES_CMD, &["-L", "INPUT", "-v", "-n"]) {
            let mut first_found = false;
            let mut second_found = false;

            if output.status.success() {
                let std_output = String::from_utf8_lossy(&output.stdout).to_string();
                for line in std_output.lines() {
                    if !first_found && line.contains(first.0) && line.contains(first.1) {
                        first_found = true;
                        continue;
                    }

                    if first_found && line.contains(second.0) && line.contains(second.1) {
                        second_found = true;
                        break;
                    }
                }
            }

            if first_found && second_found {
                result.status = CheckStatus::PASS;
            } else {
                result.error = "Unable to find expected iptables INPUT values".to_string();
                result.status = CheckStatus::FAIL;
                return result;
            }
        } else {
            result.error = "unable to verify iptables INPUT settings".to_string();
        }

        if let Some(found) = look_for_string_in_output(
            sac,
            IPTABLES_CMD,
            &["-L", "OUTPUT", "-v", "-n"],
            "ACCEPT     all  --  *      lo      0.0.0.0/0            0.0.0.0/0",
        ) {
            if !found {
                result.error = "iptables OUTPUT rule not found".to_string();
                result.status = CheckStatus::FAIL;
            } else {
                result.status = CheckStatus::PASS;
            }
        } else {
            result.error =
                "unable to parse iptables OUTPUT rules to verify loopback policy".to_string();
        }

        result
    }

    fn metadata(&self) -> CheckerMetadata {
        CheckerMetadata {
            title: "Ensure IPv4 loopback traffic is configured".to_string(),
            id: "3.4.1.2".to_string(),
            level: 2,
            name: "br03040102".to_string(),
            mode: Mode::Automatic,
        }
    }
}

// =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<=

pub struct BR03040201Checker {}

impl Checker for BR03040201Checker {
    fn execute(&self, sac: &dyn SystemAccess) -> CheckerResult {
        let output = &[
            "Chain INPUT (policy DROP)",
            "Chain FORWARD (policy DROP)",
            "Chain OUTPUT (policy DROP)",
        ];

        check_output_contains!(
            sac,
            IP6TABLES_CMD,
            &["-L"],
            output,
            "unable to verify ip6tables settings",
            "unable to find expected ip6tables values"
        )
    }

    fn metadata(&self) -> CheckerMetadata {
        CheckerMetadata {
            title: "Ensure IPv6 default deny firewall policy".to_string(),
            id: "3.4.2.1".to_string(),
            level: 2,
            name: "br03040201".to_string(),
            mode: Mode::Automatic,
        }
    }
}

// =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<=

pub struct BR03040202Checker {}

impl Checker for BR03040202Checker {
    fn execute(&self, sac: &dyn SystemAccess) -> CheckerResult {
        let mut result = CheckerResult::default();

        // Order matters here, so need to find the first one, then look for the second one
        let first = ("ACCEPT", "--  lo     *       ::/0                 ::/0");
        let second = ("DROP", "--  *      *       ::1                  ::/0");

        if let Ok(output) = sac.command_output(IP6TABLES_CMD, &["-L", "INPUT", "-v", "-n"]) {
            let mut first_found = false;
            let mut second_found = false;

            if output.status.success() {
                let std_output = String::from_utf8_lossy(&output.stdout).to_string();
                for line in std_output.lines() {
                    if !first_found && line.contains(first.0) && line.contains(first.1) {
                        first_found = true;
                        continue;
                    }

                    if first_found && line.contains(second.0) && line.contains(second.1) {
                        second_found = true;
                        break;
                    }
                }
            }

            if first_found && second_found {
                result.status = CheckStatus::PASS;
            } else {
                result.error = "Unable to find expected iptables INPUT values".to_string();
                result.status = CheckStatus::FAIL;
                return result;
            }
        } else {
            result.error = "unable to verify iptables INPUT settings".to_string();
        }

        if let Some(found) = look_for_string_in_output(
            sac,
            IP6TABLES_CMD,
            &["-L", "OUTPUT", "-v", "-n"],
            "ACCEPT     all  --  *      lo      ::/0                 ::/0",
        ) {
            if !found {
                result.error = "iptables OUTPUT rule not found".to_string();
                result.status = CheckStatus::FAIL;
            } else {
                result.status = CheckStatus::PASS;
            }
        } else {
            result.error =
                "unable to parse iptables OUTPUT rules to verify loopback policy".to_string();
        }

        result
    }

    fn metadata(&self) -> CheckerMetadata {
        CheckerMetadata {
            title: "Ensure IPv6 loopback traffic is configured".to_string(),
            id: "3.4.2.2".to_string(),
            level: 2,
            name: "br03040202".to_string(),
            mode: Mode::Automatic,
        }
    }
}

// =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<=

pub struct BR04010101Checker {}

impl Checker for BR04010101Checker {
    fn execute(&self, sac: &dyn SystemAccess) -> CheckerResult {
        check_file_contains!(
            sac,
            JOURNALD_CONF_FILE,
            &["Storage=persistent"],
            "unable to verify journald settings",
            "journald is not configured"
        )
    }

    fn metadata(&self) -> CheckerMetadata {
        CheckerMetadata {
            title: "Ensure journald is configured to write logs to persistent disk".to_string(),
            id: "4.1.1.1".to_string(),
            level: 1,
            name: "br04010101".to_string(),
            mode: Mode::Automatic,
        }
    }
}

// =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<=

pub struct BR04010200Checker {}

impl Checker for BR04010200Checker {
    fn execute(&self, _: &dyn SystemAccess) -> CheckerResult {
        // Default the result to report success
        let mut result = {
            CheckerResult {
                status: CheckStatus::PASS,
                ..Default::default()
            }
        };

        // Recursively walk over all files in /var/log/journal and check perms
        for file in WalkDir::new("/var/log/journal")
            .into_iter()
            .filter_map(|file| file.ok())
        {
            if let Ok(metadata) = file.metadata() {
                if !metadata.is_file() {
                    continue;
                }

                if (metadata.permissions().mode() & 0b111) > 0 {
                    result.error = format!("file {:?} has permissions for 'other'", file.path());
                    result.status = CheckStatus::FAIL;
                    break;
                }
            }
        }

        result
    }

    fn metadata(&self) -> CheckerMetadata {
        CheckerMetadata {
            title: "Ensure permissions on journal files are configured".to_string(),
            id: "4.1.2".to_string(),
            level: 1,
            name: "br04010200".to_string(),
            mode: Mode::Automatic,
        }
    }
}

#[cfg(test)]
mod tests {

    use crate::checks::*;
    use bloodhound::results::{CheckStatus, Checker};
    use bloodhound::system_access::UnitTestSystemAccess;
    use std::process::{ExitStatus, Output};

    // BR01010101Checker tests
    #[test]
    pub fn test_br01010101checker_proc_modules_missing() {
        let sac = UnitTestSystemAccess::default();
        let checker = BR01010101Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::SKIP);
    }

    #[test]
    pub fn test_br01010101checker_udf_loaded() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_file(PROC_MODULES_FILE, "udf\n");
        let checker = BR01010101Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::FAIL);
    }

    #[test]
    pub fn test_br01010101checker_udf_not_loaded_loading_not_blocked() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_file(PROC_MODULES_FILE, "other\nmodules\n");
        sac.register_command(
            MODPROBE_CMD,
            &["-n", "-v", "udf"],
            Output {
                status: ExitStatus::default(),
                stdout: "insmod /lib/modules/kernel/fs/udf/udf.ko".into(),
                stderr: vec![],
            },
        );
        let checker = BR01010101Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::FAIL);
    }

    #[test]
    pub fn test_br01010101checker_udf_not_loaded_loading_blocked() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_file(PROC_MODULES_FILE, "other\nmodules\n");
        sac.register_command(
            MODPROBE_CMD,
            &["-n", "-v", "udf"],
            Output {
                status: ExitStatus::default(),
                stdout: "install /bin/true".into(),
                stderr: vec![],
            },
        );
        let checker = BR01010101Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::PASS);
    }

    // BR01030100Checker tests
    #[test]
    pub fn test_br01030100checker_pass() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_file(
            PROC_CMDLINE_FILE,
            "dm-mod.create=root,,,ro,0 root=/dev/dm-0 restart_on_corruption other_param=value",
        );
        let checker = BR01030100Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::PASS);
    }

    #[test]
    pub fn test_br01030100checker_missing_params() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_file(PROC_CMDLINE_FILE, "root=/dev/sda1 other_param=value");
        let checker = BR01030100Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::FAIL);
    }

    #[test]
    pub fn test_br01030100checker_file_missing() {
        let sac = UnitTestSystemAccess::default();
        let checker = BR01030100Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::SKIP);
    }

    // BR01040100Checker tests
    #[test]
    pub fn test_br01040100checker_pass() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_command(
            SYSCTL_CMD,
            &["fs.suid_dumpable"],
            Output {
                status: ExitStatus::default(),
                stdout: "fs.suid_dumpable = 0".into(),
                stderr: vec![],
            },
        );
        let checker = BR01040100Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::PASS);
    }

    #[test]
    pub fn test_br01040100checker_fail() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_command(
            SYSCTL_CMD,
            &["fs.suid_dumpable"],
            Output {
                status: ExitStatus::default(),
                stdout: "fs.suid_dumpable = 1".into(),
                stderr: vec![],
            },
        );
        let checker = BR01040100Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::FAIL);
    }

    // BR01040200Checker tests
    #[test]
    pub fn test_br01040200checker_pass() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_command(
            SYSCTL_CMD,
            &["kernel.randomize_va_space"],
            Output {
                status: ExitStatus::default(),
                stdout: "kernel.randomize_va_space = 2".into(),
                stderr: vec![],
            },
        );
        let checker = BR01040200Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::PASS);
    }

    #[test]
    pub fn test_br01040200checker_fail() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_command(
            SYSCTL_CMD,
            &["kernel.randomize_va_space"],
            Output {
                status: ExitStatus::default(),
                stdout: "kernel.randomize_va_space = 0".into(),
                stderr: vec![],
            },
        );
        let checker = BR01040200Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::FAIL);
    }

    // BR01040300Checker tests
    #[test]
    pub fn test_br01040300checker_pass() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_command(
            SYSCTL_CMD,
            &["kernel.unprivileged_bpf_disabled"],
            Output {
                status: ExitStatus::default(),
                stdout: "kernel.unprivileged_bpf_disabled = 1".into(),
                stderr: vec![],
            },
        );
        let checker = BR01040300Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::PASS);
    }

    #[test]
    pub fn test_br01040300checker_fail() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_command(
            SYSCTL_CMD,
            &["kernel.unprivileged_bpf_disabled"],
            Output {
                status: ExitStatus::default(),
                stdout: "kernel.unprivileged_bpf_disabled = 0".into(),
                stderr: vec![],
            },
        );
        let checker = BR01040300Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::FAIL);
    }

    // BR01040400Checker tests
    #[test]
    pub fn test_br01040400checker_pass() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_command(
            SYSCTL_CMD,
            &["user.max_user_namespaces"],
            Output {
                status: ExitStatus::default(),
                stdout: "user.max_user_namespaces = 0".into(),
                stderr: vec![],
            },
        );
        let checker = BR01040400Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::PASS);
    }

    #[test]
    pub fn test_br01040400checker_fail() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_command(
            SYSCTL_CMD,
            &["user.max_user_namespaces"],
            Output {
                status: ExitStatus::default(),
                stdout: "user.max_user_namespaces = 1000".into(),
                stderr: vec![],
            },
        );
        let checker = BR01040400Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::FAIL);
    }
    // BR01050100Checker tests
    #[test]
    pub fn test_br01050100checker_pass() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_command(
            SESTATUS_CMD,
            &[],
            Output {
                status: ExitStatus::default(),
                stdout: concat!(
                    "SELinux status: enabled\n",
                    "Loaded policy name: fortified\n",
                    "Current mode: enforcing\n",
                    "Mode from config file: enforcing\n",
                    "Policy MLS status: enabled\n",
                    "Policy deny_unknown status: denied\n",
                    "Memory protection checking: actual (secure)\n"
                )
                .into(),
                stderr: vec![],
            },
        );
        let checker = BR01050100Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::PASS);
    }

    #[test]
    pub fn test_br01050100checker_fail_missing_values() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_command(
            SESTATUS_CMD,
            &[],
            Output {
                status: ExitStatus::default(),
                stdout: concat!(
                    "SELinux status: disabled\n",
                    "Loaded policy name: fortified\n",
                    "Current mode: permissive\n"
                )
                .into(),
                stderr: vec![],
            },
        );
        let checker = BR01050100Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::FAIL);
    }

    #[test]
    pub fn test_br01050100checker_command_fail() {
        let sac = UnitTestSystemAccess::default();
        let checker = BR01050100Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::SKIP);
    }

    // BR01050200Checker tests
    #[test]
    pub fn test_br01050200checker_pass() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_file(LOCKDOWN_FILE, "[integrity] confidentiality");
        let checker = BR01050200Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::PASS);
    }

    #[test]
    pub fn test_br01050200checker_fail() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_file(LOCKDOWN_FILE, "none [confidentiality]");
        let checker = BR01050200Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::FAIL);
    }

    #[test]
    pub fn test_br01050200checker_file_missing() {
        let sac = UnitTestSystemAccess::default();
        let checker = BR01050200Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::SKIP);
    }

    // BR02010101Checker tests
    #[test]
    pub fn test_br02010101checker_pass() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_file(
            CHRONY_CONF_FILE,
            "pool 2.amazon.pool.ntp.org iburst\nserver time.nist.gov",
        );
        sac.register_command(
            SYSTEMCTL_CMD,
            &["show", "--property", "ActiveState", "chronyd"],
            Output {
                status: ExitStatus::default(),
                stdout: "ActiveState=active".into(),
                stderr: vec![],
            },
        );
        let checker = BR02010101Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::PASS);
    }

    #[test]
    pub fn test_br02010101checker_fail_no_pool() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_file(
            CHRONY_CONF_FILE,
            "server time.nist.gov\ndriftfile /var/lib/chrony/drift",
        );
        let checker = BR02010101Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::FAIL);
    }

    #[test]
    pub fn test_br02010101checker_fail_service_inactive() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_file(CHRONY_CONF_FILE, "pool 2.amazon.pool.ntp.org iburst");
        sac.register_command(
            SYSTEMCTL_CMD,
            &["show", "--property", "ActiveState", "chronyd"],
            Output {
                status: ExitStatus::default(),
                stdout: "ActiveState=inactive".into(),
                stderr: vec![],
            },
        );
        let checker = BR02010101Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::FAIL);
    }

    #[test]
    pub fn test_br02010101checker_file_missing() {
        let sac = UnitTestSystemAccess::default();
        let checker = BR02010101Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::SKIP);
    }
    // BR03010100Checker tests
    #[test]
    pub fn test_br03010100checker_pass() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_command(
            SYSCTL_CMD,
            &[
                "net.ipv4.conf.all.send_redirects",
                "net.ipv4.conf.default.send_redirects",
            ],
            Output {
                status: ExitStatus::default(),
                stdout: concat!(
                    "net.ipv4.conf.all.send_redirects = 0\n",
                    "net.ipv4.conf.default.send_redirects = 0\n"
                )
                .into(),
                stderr: vec![],
            },
        );
        let checker = BR03010100Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::PASS);
    }

    #[test]
    pub fn test_br03010100checker_fail() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_command(
            SYSCTL_CMD,
            &[
                "net.ipv4.conf.all.send_redirects",
                "net.ipv4.conf.default.send_redirects",
            ],
            Output {
                status: ExitStatus::default(),
                stdout: concat!(
                    "net.ipv4.conf.all.send_redirects = 1\n",
                    "net.ipv4.conf.default.send_redirects = 0\n"
                )
                .into(),
                stderr: vec![],
            },
        );
        let checker = BR03010100Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::FAIL);
    }

    // BR03020100Checker tests
    #[test]
    pub fn test_br03020100checker_pass() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_command(
            SYSCTL_CMD,
            &[
                "net.ipv4.conf.all.accept_source_route",
                "net.ipv4.conf.default.accept_source_route",
                "net.ipv6.conf.all.accept_source_route",
                "net.ipv6.conf.default.accept_source_route",
            ],
            Output {
                status: ExitStatus::default(),
                stdout: concat!(
                    "net.ipv4.conf.all.accept_source_route = 0\n",
                    "net.ipv4.conf.default.accept_source_route = 0\n",
                    "net.ipv6.conf.all.accept_source_route = 0\n",
                    "net.ipv6.conf.default.accept_source_route = 0\n"
                )
                .into(),
                stderr: vec![],
            },
        );
        let checker = BR03020100Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::PASS);
    }

    #[test]
    pub fn test_br03020100checker_fail() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_command(
            SYSCTL_CMD,
            &[
                "net.ipv4.conf.all.accept_source_route",
                "net.ipv4.conf.default.accept_source_route",
                "net.ipv6.conf.all.accept_source_route",
                "net.ipv6.conf.default.accept_source_route",
            ],
            Output {
                status: ExitStatus::default(),
                stdout: concat!(
                    "net.ipv4.conf.all.accept_source_route = 1\n",
                    "net.ipv4.conf.default.accept_source_route = 0\n",
                    "net.ipv6.conf.all.accept_source_route = 0\n",
                    "net.ipv6.conf.default.accept_source_route = 0\n"
                )
                .into(),
                stderr: vec![],
            },
        );
        let checker = BR03020100Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::FAIL);
    }

    // BR03020200Checker tests
    #[test]
    pub fn test_br03020200checker_pass() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_command(
            SYSCTL_CMD,
            &[
                "net.ipv4.conf.all.accept_redirects",
                "net.ipv4.conf.default.accept_redirects",
                "net.ipv6.conf.all.accept_redirects",
                "net.ipv6.conf.default.accept_redirects",
            ],
            Output {
                status: ExitStatus::default(),
                stdout: concat!(
                    "net.ipv4.conf.all.accept_redirects = 0\n",
                    "net.ipv4.conf.default.accept_redirects = 0\n",
                    "net.ipv6.conf.all.accept_redirects = 0\n",
                    "net.ipv6.conf.default.accept_redirects = 0\n"
                )
                .into(),
                stderr: vec![],
            },
        );
        let checker = BR03020200Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::PASS);
    }

    #[test]
    pub fn test_br03020200checker_fail() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_command(
            SYSCTL_CMD,
            &[
                "net.ipv4.conf.all.accept_redirects",
                "net.ipv4.conf.default.accept_redirects",
                "net.ipv6.conf.all.accept_redirects",
                "net.ipv6.conf.default.accept_redirects",
            ],
            Output {
                status: ExitStatus::default(),
                stdout: concat!(
                    "net.ipv4.conf.all.accept_redirects = 0\n",
                    "net.ipv4.conf.default.accept_redirects = 1\n",
                    "net.ipv6.conf.all.accept_redirects = 0\n",
                    "net.ipv6.conf.default.accept_redirects = 0\n"
                )
                .into(),
                stderr: vec![],
            },
        );
        let checker = BR03020200Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::FAIL);
    }

    // BR03020300Checker tests
    #[test]
    pub fn test_br03020300checker_pass() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_command(
            SYSCTL_CMD,
            &[
                "net.ipv4.conf.all.secure_redirects",
                "net.ipv4.conf.default.secure_redirects",
            ],
            Output {
                status: ExitStatus::default(),
                stdout: concat!(
                    "net.ipv4.conf.all.secure_redirects = 0\n",
                    "net.ipv4.conf.default.secure_redirects = 0\n"
                )
                .into(),
                stderr: vec![],
            },
        );
        let checker = BR03020300Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::PASS);
    }

    #[test]
    pub fn test_br03020300checker_fail() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_command(
            SYSCTL_CMD,
            &[
                "net.ipv4.conf.all.secure_redirects",
                "net.ipv4.conf.default.secure_redirects",
            ],
            Output {
                status: ExitStatus::default(),
                stdout: concat!(
                    "net.ipv4.conf.all.secure_redirects = 1\n",
                    "net.ipv4.conf.default.secure_redirects = 0\n"
                )
                .into(),
                stderr: vec![],
            },
        );
        let checker = BR03020300Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::FAIL);
    }

    // BR03020400Checker tests
    #[test]
    pub fn test_br03020400checker_pass() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_command(
            SYSCTL_CMD,
            &[
                "net.ipv4.conf.all.log_martians",
                "net.ipv4.conf.default.log_martians",
            ],
            Output {
                status: ExitStatus::default(),
                stdout: concat!(
                    "net.ipv4.conf.all.log_martians = 1\n",
                    "net.ipv4.conf.default.log_martians = 1\n"
                )
                .into(),
                stderr: vec![],
            },
        );
        let checker = BR03020400Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::PASS);
    }

    #[test]
    pub fn test_br03020400checker_fail() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_command(
            SYSCTL_CMD,
            &[
                "net.ipv4.conf.all.log_martians",
                "net.ipv4.conf.default.log_martians",
            ],
            Output {
                status: ExitStatus::default(),
                stdout: concat!(
                    "net.ipv4.conf.all.log_martians = 0\n",
                    "net.ipv4.conf.default.log_martians = 1\n"
                )
                .into(),
                stderr: vec![],
            },
        );
        let checker = BR03020400Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::FAIL);
    }
    // BR03020500Checker tests
    #[test]
    pub fn test_br03020500checker_pass() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_command(
            SYSCTL_CMD,
            &["net.ipv4.icmp_echo_ignore_broadcasts"],
            Output {
                status: ExitStatus::default(),
                stdout: "net.ipv4.icmp_echo_ignore_broadcasts = 1".into(),
                stderr: vec![],
            },
        );
        let checker = BR03020500Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::PASS);
    }

    #[test]
    pub fn test_br03020500checker_fail() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_command(
            SYSCTL_CMD,
            &["net.ipv4.icmp_echo_ignore_broadcasts"],
            Output {
                status: ExitStatus::default(),
                stdout: "net.ipv4.icmp_echo_ignore_broadcasts = 0".into(),
                stderr: vec![],
            },
        );
        let checker = BR03020500Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::FAIL);
    }

    // BR03020600Checker tests
    #[test]
    pub fn test_br03020600checker_pass() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_command(
            SYSCTL_CMD,
            &["net.ipv4.icmp_ignore_bogus_error_responses"],
            Output {
                status: ExitStatus::default(),
                stdout: "net.ipv4.icmp_ignore_bogus_error_responses = 1".into(),
                stderr: vec![],
            },
        );
        let checker = BR03020600Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::PASS);
    }

    #[test]
    pub fn test_br03020600checker_fail() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_command(
            SYSCTL_CMD,
            &["net.ipv4.icmp_ignore_bogus_error_responses"],
            Output {
                status: ExitStatus::default(),
                stdout: "net.ipv4.icmp_ignore_bogus_error_responses = 0".into(),
                stderr: vec![],
            },
        );
        let checker = BR03020600Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::FAIL);
    }

    // BR03020700Checker tests
    #[test]
    pub fn test_br03020700checker_pass() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_command(
            SYSCTL_CMD,
            &["net.ipv4.tcp_syncookies"],
            Output {
                status: ExitStatus::default(),
                stdout: "net.ipv4.tcp_syncookies = 1".into(),
                stderr: vec![],
            },
        );
        let checker = BR03020700Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::PASS);
    }

    #[test]
    pub fn test_br03020700checker_fail() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_command(
            SYSCTL_CMD,
            &["net.ipv4.tcp_syncookies"],
            Output {
                status: ExitStatus::default(),
                stdout: "net.ipv4.tcp_syncookies = 0".into(),
                stderr: vec![],
            },
        );
        let checker = BR03020700Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::FAIL);
    }

    // BR03030100Checker tests
    #[test]
    pub fn test_br03030100checker_pass() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_file(PROC_MODULES_FILE, "other\nmodules\n");
        sac.register_command(
            MODPROBE_CMD,
            &["-n", "-v", "sctp"],
            Output {
                status: ExitStatus::default(),
                stdout: "install /bin/true".into(),
                stderr: vec![],
            },
        );
        let checker = BR03030100Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::PASS);
    }

    #[test]
    pub fn test_br03030100checker_fail_sctp_loaded() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_file(
            PROC_MODULES_FILE,
            "sctp 139264 0 - Live 0xffffffffc05e1000\n",
        );
        let checker = BR03030100Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::FAIL);
    }

    #[test]
    pub fn test_br03030100checker_fail_sctp_not_blocked() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_file(PROC_MODULES_FILE, "other\nmodules\n");
        sac.register_command(
            MODPROBE_CMD,
            &["-n", "-v", "sctp"],
            Output {
                status: ExitStatus::default(),
                stdout: "insmod /lib/modules/kernel/net/sctp/sctp.ko".into(),
                stderr: vec![],
            },
        );
        let checker = BR03030100Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::FAIL);
    }

    #[test]
    pub fn test_br03030100checker_proc_modules_missing() {
        let sac = UnitTestSystemAccess::default();
        let checker = BR03030100Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::SKIP);
    }
    // BR03040101Checker tests
    #[test]
    pub fn test_br03040101checker_pass() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_command(
            IPTABLES_CMD,
            &["-L"],
            Output {
                status: ExitStatus::default(),
                stdout: concat!(
                    "Chain INPUT (policy DROP)\n",
                    "Chain FORWARD (policy DROP)\n",
                    "Chain OUTPUT (policy DROP)\n"
                )
                .into(),
                stderr: vec![],
            },
        );
        let checker = BR03040101Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::PASS);
    }

    #[test]
    pub fn test_br03040101checker_fail() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_command(
            IPTABLES_CMD,
            &["-L"],
            Output {
                status: ExitStatus::default(),
                stdout: concat!(
                    "Chain INPUT (policy ACCEPT)\n",
                    "Chain FORWARD (policy DROP)\n",
                    "Chain OUTPUT (policy DROP)\n"
                )
                .into(),
                stderr: vec![],
            },
        );
        let checker = BR03040101Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::FAIL);
    }

    // BR03040102Checker tests
    #[test]
    pub fn test_br03040102checker_pass() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_command(
            IPTABLES_CMD,
            &["-L", "INPUT", "-v", "-n"],
            Output {
                status: ExitStatus::default(),
                stdout: concat!(
                    "Chain INPUT (policy DROP 0 packets, 0 bytes)\n",
                    " pkts bytes target     prot opt in     out     source               destination\n",
                    "    0     0 ACCEPT     all  --  lo     *       0.0.0.0/0            0.0.0.0/0\n",
                    "    0     0 DROP       all  --  *      *       127.0.0.0/8          0.0.0.0/0\n"
                ).into(),
                stderr: vec![],
            },
        );
        sac.register_command(
            IPTABLES_CMD,
            &["-L", "OUTPUT", "-v", "-n"],
            Output {
                status: ExitStatus::default(),
                stdout: concat!(
                    "Chain OUTPUT (policy DROP 0 packets, 0 bytes)\n",
                    " pkts bytes target     prot opt in     out     source               destination\n",
                    "    0     0 ACCEPT     all  --  *      lo      0.0.0.0/0            0.0.0.0/0\n"
                ).into(),
                stderr: vec![],
            },
        );
        let checker = BR03040102Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::PASS);
    }

    #[test]
    pub fn test_br03040102checker_fail_missing_input_rules() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_command(
            IPTABLES_CMD,
            &["-L", "INPUT", "-v", "-n"],
            Output {
                status: ExitStatus::default(),
                stdout: concat!(
                    "Chain INPUT (policy DROP 0 packets, 0 bytes)\n",
                    " pkts bytes target     prot opt in     out     source               destination\n"
                ).into(),
                stderr: vec![],
            },
        );
        let checker = BR03040102Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::FAIL);
    }

    #[test]
    pub fn test_br03040102checker_fail_missing_output_rule() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_command(
            IPTABLES_CMD,
            &["-L", "INPUT", "-v", "-n"],
            Output {
                status: ExitStatus::default(),
                stdout: concat!(
                    "Chain INPUT (policy DROP 0 packets, 0 bytes)\n",
                    " pkts bytes target     prot opt in     out     source               destination\n",
                    "    0     0 ACCEPT     all  --  lo     *       0.0.0.0/0            0.0.0.0/0\n",
                    "    0     0 DROP       all  --  *      *       127.0.0.0/8          0.0.0.0/0\n"
                ).into(),
                stderr: vec![],
            },
        );
        sac.register_command(
            IPTABLES_CMD,
            &["-L", "OUTPUT", "-v", "-n"],
            Output {
                status: ExitStatus::default(),
                stdout: concat!(
                    "Chain OUTPUT (policy DROP 0 packets, 0 bytes)\n",
                    " pkts bytes target     prot opt in     out     source               destination\n"
                ).into(),
                stderr: vec![],
            },
        );
        let checker = BR03040102Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::FAIL);
    }

    // BR03040201Checker tests
    #[test]
    pub fn test_br03040201checker_pass() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_command(
            IP6TABLES_CMD,
            &["-L"],
            Output {
                status: ExitStatus::default(),
                stdout: concat!(
                    "Chain INPUT (policy DROP)\n",
                    "Chain FORWARD (policy DROP)\n",
                    "Chain OUTPUT (policy DROP)\n"
                )
                .into(),
                stderr: vec![],
            },
        );
        let checker = BR03040201Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::PASS);
    }

    #[test]
    pub fn test_br03040201checker_fail() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_command(
            IP6TABLES_CMD,
            &["-L"],
            Output {
                status: ExitStatus::default(),
                stdout: concat!(
                    "Chain INPUT (policy ACCEPT)\n",
                    "Chain FORWARD (policy DROP)\n",
                    "Chain OUTPUT (policy DROP)\n"
                )
                .into(),
                stderr: vec![],
            },
        );
        let checker = BR03040201Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::FAIL);
    }

    // BR03040202Checker tests
    #[test]
    pub fn test_br03040202checker_pass() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_command(
            IP6TABLES_CMD,
            &["-L", "INPUT", "-v", "-n"],
            Output {
                status: ExitStatus::default(),
                stdout: concat!(
                    "Chain INPUT (policy DROP 0 packets, 0 bytes)\n",
                    " pkts bytes target     prot opt in     out     source               destination\n",
                    "    0     0 ACCEPT     all  --  lo     *       ::/0                 ::/0\n",
                    "    0     0 DROP       all  --  *      *       ::1                  ::/0\n"
                ).into(),
                stderr: vec![],
            },
        );
        sac.register_command(
            IP6TABLES_CMD,
            &["-L", "OUTPUT", "-v", "-n"],
            Output {
                status: ExitStatus::default(),
                stdout: concat!(
                    "Chain OUTPUT (policy DROP 0 packets, 0 bytes)\n",
                    " pkts bytes target     prot opt in     out     source               destination\n",
                    "    0     0 ACCEPT     all  --  *      lo      ::/0                 ::/0\n"
                ).into(),
                stderr: vec![],
            },
        );
        let checker = BR03040202Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::PASS);
    }

    #[test]
    pub fn test_br03040202checker_fail_missing_input_rules() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_command(
            IP6TABLES_CMD,
            &["-L", "INPUT", "-v", "-n"],
            Output {
                status: ExitStatus::default(),
                stdout: concat!(
                    "Chain INPUT (policy DROP 0 packets, 0 bytes)\n",
                    " pkts bytes target     prot opt in     out     source               destination\n"
                ).into(),
                stderr: vec![],
            },
        );
        let checker = BR03040202Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::FAIL);
    }

    #[test]
    pub fn test_br03040202checker_fail_missing_output_rule() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_command(
            IP6TABLES_CMD,
            &["-L", "INPUT", "-v", "-n"],
            Output {
                status: ExitStatus::default(),
                stdout: concat!(
                    "Chain INPUT (policy DROP 0 packets, 0 bytes)\n",
                    " pkts bytes target     prot opt in     out     source               destination\n",
                    "    0     0 ACCEPT     all  --  lo     *       ::/0                 ::/0\n",
                    "    0     0 DROP       all  --  *      *       ::1                  ::/0\n"
                ).into(),
                stderr: vec![],
            },
        );
        sac.register_command(
            IP6TABLES_CMD,
            &["-L", "OUTPUT", "-v", "-n"],
            Output {
                status: ExitStatus::default(),
                stdout: concat!(
                    "Chain OUTPUT (policy DROP 0 packets, 0 bytes)\n",
                    " pkts bytes target     prot opt in     out     source               destination\n"
                ).into(),
                stderr: vec![],
            },
        );
        let checker = BR03040202Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::FAIL);
    }
    // BR04010101Checker tests
    #[test]
    pub fn test_br04010101checker_pass() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_file(
            JOURNALD_CONF_FILE,
            "Storage=persistent\nCompress=yes\nSeal=yes",
        );
        let checker = BR04010101Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::PASS);
    }

    #[test]
    pub fn test_br04010101checker_fail() {
        let mut sac = UnitTestSystemAccess::default();
        sac.register_file(
            JOURNALD_CONF_FILE,
            "Storage=volatile\nCompress=yes\nSeal=yes",
        );
        let checker = BR04010101Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::FAIL);
    }

    #[test]
    pub fn test_br04010101checker_file_missing() {
        let sac = UnitTestSystemAccess::default();
        let checker = BR04010101Checker {};
        let result = checker.execute(&sac);
        assert_eq!(result.status, CheckStatus::SKIP);
    }
}
