//! Integration tests for schnauzer multicall binary
//!
//! These tests validate that the multicall binary correctly dispatches to the appropriate
//! CLI module based on how it's invoked, including symlink behavior.

use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

/// Test harness for multicall binary tests
struct MulticallTest {
    /// Path to the original binary
    binary_path: &'static str,
    /// Temporary directory for test files
    temp_dir: TempDir,
}

impl MulticallTest {
    /// Create a new test harness
    fn new() -> Self {
        Self {
            binary_path: env!("CARGO_BIN_EXE_schnauzer"),
            temp_dir: TempDir::new().expect("Failed to create temp directory"),
        }
    }

    /// Create a symlink to the binary with the given name
    fn create_symlink(&self, name: &str) -> PathBuf {
        let link_path = self.temp_dir.path().join(name);
        symlink(self.binary_path, &link_path)
            .unwrap_or_else(|_| panic!("Failed to create {name} symlink"));
        link_path
    }

    /// Run a command with the given binary path and arguments
    fn run_command(&self, binary_path: &Path, args: &[&str]) -> Output {
        Command::new(binary_path)
            .args(args)
            .output()
            .unwrap_or_else(|_| panic!("Failed to execute {binary_path:?} with args {args:?}"))
    }

    /// Check if the output contains v1-specific usage message
    fn is_v1_output(&self, output: &Output) -> bool {
        let stderr = String::from_utf8_lossy(&output.stderr);
        stderr.contains("Usage:") && stderr.contains("SETTING_KEY")
    }

    /// Check if the output contains v2-specific patterns
    fn is_v2_output(&self, output: &Output) -> bool {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);

        // v2 has different usage patterns - look for v2-specific elements
        stderr.contains("render-file")
            || stderr.contains("render")
            || stdout.contains("--log-level") && !self.is_v1_output(output)
    }
}

#[test]
fn test_symlink_dispatch() {
    // Given A multicall binary and symlinks with different names
    let test = MulticallTest::new();
    let schnauzer_link = test.create_symlink("schnauzer");
    let schnauzer_v2_link = test.create_symlink("schnauzer-v2");

    // When Invoking each symlink with --help
    let output_v1 = test.run_command(&schnauzer_link, &["--help"]);
    let output_v2 = test.run_command(&schnauzer_v2_link, &["--help"]);

    // Then Each should route to the correct implementation
    assert!(
        test.is_v1_output(&output_v1),
        "schnauzer should route to v1"
    );

    assert!(
        test.is_v2_output(&output_v2),
        "schnauzer-v2 should route to v2"
    );

    // The outputs should be different, indicating different code paths
    let stderr_v1 = String::from_utf8_lossy(&output_v1.stderr);
    let stderr_v2 = String::from_utf8_lossy(&output_v2.stderr);
    assert_ne!(
        stderr_v1, stderr_v2,
        "v1 and v2 should produce different outputs"
    );
}

#[test]
fn test_unrecognized_program_name_defaults_to_v2() {
    // Given A multicall binary with an unrecognized symlink name
    let test = MulticallTest::new();
    let unknown_link = test.create_symlink("unknown-tool");

    // When Invoking the binary with the unknown name
    let output = test.run_command(&unknown_link, &["--help"]);

    // Then it should default to v2 behavior
    assert!(
        test.is_v2_output(&output),
        "Unknown tool name should default to v2"
    );
}
