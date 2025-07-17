use bloodhound::results::{CheckStatus, Checker, CheckerMetadata, CheckerResult, Mode};
use bloodhound::system_access::SystemAccess;
use bloodhound::*;

const CRYPTO_FIPS_ENABLED: &str = "/proc/sys/crypto/fips_enabled";
const EXPECTED_FIPS_ENABLED: &str = "1";

const CRYPTO_FIPS_NAME: &str = "/proc/sys/crypto/fips_name";
const EXPECTED_FIPS_NAME: &str = "Amazon Linux 2023 Kernel Cryptographic API";

const FIPS_KERNEL_CHECK_MARKER: &str = "/etc/.fips-kernel-check-passed";
const FIPS_MODULE_CHECK_MARKER: &str = "/etc/.fips-module-check-passed";

// =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<=

pub struct FIPS01000000Checker {}

impl Checker for FIPS01000000Checker {
    fn execute(&self, sac: &dyn SystemAccess) -> CheckerResult {
        check_file_contains!(
            sac,
            CRYPTO_FIPS_ENABLED,
            &[EXPECTED_FIPS_ENABLED],
            format!("{CRYPTO_FIPS_ENABLED} != {EXPECTED_FIPS_ENABLED}"),
            format!("{CRYPTO_FIPS_ENABLED} not found")
        )
    }

    fn metadata(&self) -> CheckerMetadata {
        CheckerMetadata {
            title: "FIPS mode is enabled.".to_string(),
            id: "1.0".to_string(),
            level: 0,
            name: "fips01000000".to_string(),
            mode: Mode::Automatic,
        }
    }
}

// =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<=

pub struct FIPS01010000Checker {}

impl Checker for FIPS01010000Checker {
    fn execute(&self, sac: &dyn SystemAccess) -> CheckerResult {
        check_file_contains!(
            sac,
            CRYPTO_FIPS_NAME,
            &[EXPECTED_FIPS_NAME],
            format!("{CRYPTO_FIPS_NAME} != '{EXPECTED_FIPS_NAME}'"),
            format!("{CRYPTO_FIPS_NAME} not found")
        )
    }

    fn metadata(&self) -> CheckerMetadata {
        CheckerMetadata {
            title: format!("FIPS module is {EXPECTED_FIPS_NAME}.").to_string(),
            id: "1.1".to_string(),
            level: 0,
            name: "fips01010000".to_string(),
            mode: Mode::Automatic,
        }
    }
}

// =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<= =>o.o<=

pub struct FIPS01020000Checker {}

impl Checker for FIPS01020000Checker {
    fn execute(&self, sac: &dyn SystemAccess) -> CheckerResult {
        let result = check_file_exists!(
            sac,
            FIPS_KERNEL_CHECK_MARKER,
            format!("{FIPS_KERNEL_CHECK_MARKER} not found")
        );

        // Check if we need to continue
        if result.status == CheckStatus::FAIL {
            return result;
        }

        check_file_exists!(
            sac,
            FIPS_MODULE_CHECK_MARKER,
            format!("{FIPS_MODULE_CHECK_MARKER} not found")
        )
    }

    fn metadata(&self) -> CheckerMetadata {
        CheckerMetadata {
            title: "FIPS self-tests passed.".to_string(),
            id: "1.2".to_string(),
            level: 0,
            name: "fips01020000".to_string(),
            mode: Mode::Automatic,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::checks::*;
    use bloodhound::results::{CheckStatus, Checker};
    use bloodhound::system_access::UnitTestSystemAccess;

    #[test]
    pub fn test_fips01000000checker_missing_file() {
        let usac = UnitTestSystemAccess::default();
        let checker = FIPS01000000Checker {};
        let result = checker.execute(&usac);
        // skip the test if /proc/sys/crypto/fips_enabled is missing
        assert_eq!(result.status, CheckStatus::SKIP);
    }
    #[test]
    pub fn test_fips01000000checker_fips_disabled() {
        let mut usac = UnitTestSystemAccess::default();
        usac.register_file(CRYPTO_FIPS_ENABLED, "0");
        let checker = FIPS01000000Checker {};
        let result = checker.execute(&usac);
        assert_eq!(result.status, CheckStatus::FAIL);
    }
    #[test]
    pub fn test_fips01000000checker_fips_enabled() {
        let mut usac = UnitTestSystemAccess::default();
        usac.register_file(CRYPTO_FIPS_ENABLED, EXPECTED_FIPS_ENABLED);
        let checker = FIPS01000000Checker {};
        let result = checker.execute(&usac);
        assert_eq!(result.status, CheckStatus::PASS);
    }

    #[test]
    pub fn test_fips01010000checker_missing_fips_name() {
        let usac = UnitTestSystemAccess::default();
        let checker = FIPS01010000Checker {};
        let result = checker.execute(&usac);
        assert_eq!(result.status, CheckStatus::SKIP);
    }

    #[test]
    pub fn test_fips01010000checker_wrong_fips_name() {
        let mut usac = UnitTestSystemAccess::default();
        usac.register_file(CRYPTO_FIPS_NAME, "some wrong name");
        let checker = FIPS01010000Checker {};
        let result = checker.execute(&usac);
        assert_eq!(result.status, CheckStatus::FAIL);
    }

    #[test]
    pub fn test_fips01010000checker_passing() {
        let mut usac = UnitTestSystemAccess::default();
        usac.register_file(CRYPTO_FIPS_NAME, EXPECTED_FIPS_NAME);
        let checker = FIPS01010000Checker {};
        let result = checker.execute(&usac);
        assert_eq!(result.status, CheckStatus::PASS);
    }

    #[test]
    pub fn test_fips01020000checker_missing_module_check_marker() {
        let mut usac = UnitTestSystemAccess::default();
        usac.register_file(FIPS_KERNEL_CHECK_MARKER, "1");
        let checker = FIPS01020000Checker {};
        let result = checker.execute(&usac);
        assert_eq!(result.status, CheckStatus::FAIL);
    }
    #[test]
    pub fn test_fips01020000checker_missing_kernel_check_marker() {
        let mut usac = UnitTestSystemAccess::default();
        usac.register_file(FIPS_MODULE_CHECK_MARKER, "1");
        let checker = FIPS01020000Checker {};
        let result = checker.execute(&usac);
        assert_eq!(result.status, CheckStatus::FAIL);
    }
    #[test]
    pub fn test_fips01020000checker_passing() {
        let mut usac = UnitTestSystemAccess::default();
        usac.register_file(FIPS_KERNEL_CHECK_MARKER, "1");
        usac.register_file(FIPS_MODULE_CHECK_MARKER, "1");
        let checker = FIPS01020000Checker {};
        let result = checker.execute(&usac);
        assert_eq!(result.status, CheckStatus::PASS);
    }
}
