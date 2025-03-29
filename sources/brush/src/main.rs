/*!
# Introduction

*brush* is a sanitizing pseudo-shell. Its purpose is to parse `sh -c '<program> ...'` invocations to
decide whether the requested program should be allowed to run. It splits its third argument based
on POSIX shell rules. Any additional input validation is left up to the invoked program.

Programs can be allowed by creating a symlink in the `allowed-programs` directory:
```bash
/usr/libexec/brush/allowed-programs/my-program -> ../path/to/program
```

In most cases the symlink target should be in `/usr/bin` or `/usr/sbin`, since *brush* will use
those paths to find the requested program if it is given as a bare filename.

If the requested program is allowed, *brush* will execute it directly and never return. Otherwise,
it will print an error summary and return with exit code 127 to indicate that the command could not
be executed.

Arguments for programs can be further restricted with a configuration file in this directory:
```bash
/usr/share/brush/my-program.toml
```

Both required arguments and blocked arguments can be specified:
```toml
# Required args must appear in positional order.
required-args = ["first", "second", "third"]

# Blocked args must not appear.
blocked-args = ["--spill-secrets"]
```
*/

use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::ErrorKind;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use serde::{Deserialize, Serialize};
use shlex::Shlex;
use snafu::{prelude::*, FromString, Report, ResultExt, Whatever};
use which::CanonicalPath;

type Result<T> = std::result::Result<T, Whatever>;

// These should be read-only directories to prevent trivially altering either the set of allowed
// programs, or the arguments that are required or blocked.
static ALLOWED_PROGRAMS_DIR: &str = "/usr/libexec/brush/allowed-programs";
static CONFIG_DIR: &str = "/usr/share/brush";

// These are the paths that will be searched if the program is requested by name.
static ALLOWED_PATHS: &str = "/usr/sbin:/usr/bin";

// Ensure that the provided arguments conform to the `sh -c '...'` interface, parse the third
// argument with shlex, and return the program name and arguments.
fn parse_args(args: Vec<String>) -> Result<(String, Vec<String>)> {
    let argc = args.len();
    let mut args_iter = args.into_iter();
    ensure_whatever!(argc == 3, "expected 3 args, found {argc}");

    let arg0 = args_iter.next().unwrap();
    ensure_whatever!(arg0 == "sh", "expected first arg 'sh', not '{arg0}'");

    let arg1 = args_iter.next().unwrap();
    ensure_whatever!(arg1 == "-c", "expected second arg '-c', not '{arg1}'");

    let arg2 = args_iter.next().unwrap();
    let mut split_input = Shlex::new(&arg2);
    let split_arg0 = split_input
        .next()
        .with_whatever_context(|| "failed to identify requested program from shell input")?;
    let split_argv = split_input.collect();

    Ok((split_arg0, split_argv))
}

// Find the canonical path to the provided program, which can be relative or absolute, resolving
// any symlinks or relative path components.
fn resolve_program(program: impl AsRef<Path>, paths: impl AsRef<str>) -> Result<CanonicalPath> {
    let program = program.as_ref();
    let paths = paths.as_ref();
    let cwd = std::env::current_dir().unwrap_or_default();
    which::CanonicalPath::new_in(program, Some(paths), cwd)
        .with_whatever_context(|_| format!("failed to locate '{}' in PATH", program.display()))
}

// Find the canonical path to the provided symlink, resolving any intermediate symlinks or relative
// path components.
fn resolve_symlink(symlink: impl AsRef<Path>) -> Result<PathBuf> {
    let symlink = symlink.as_ref();
    fs::canonicalize(symlink)
        .with_whatever_context(|_| format!("failed to canonicalize '{}'", symlink.display()))
}

// Helper function to extract the filename for the given path.
fn filename(path: impl AsRef<Path>) -> Result<OsString> {
    let path = path.as_ref();
    let path = path
        .file_name()
        .with_whatever_context(|| format!("failed to get file name for '{}'", path.display()))?
        .to_os_string();
    Ok(path)
}

// Verify that the program is allowed. If successful, it will return the canonical path to the
// allowed program, along with the filename that should be used as arg0. This allows multi-call
// binaries such as `coreutils` to know which personality to use upon execution.
fn verify_program(
    allowed_programs_dir: impl AsRef<OsStr>,
    allowed_paths: impl AsRef<str>,
    program: impl AsRef<Path>,
) -> Result<(PathBuf, OsString)> {
    let requested_program_path = program.as_ref();
    let requested_program_name = filename(requested_program_path)?;
    let allowed_program_path: PathBuf = [allowed_programs_dir.as_ref(), &requested_program_name]
        .iter()
        .collect();

    let resolved_program_path = resolve_program(requested_program_path, allowed_paths)
        .with_whatever_context(|_| {
            format!(
                "'{}' could not be found",
                requested_program_name.to_string_lossy()
            )
        })?;

    let verified_program_path =
        resolve_symlink(&allowed_program_path).with_whatever_context(|_| {
            format!(
                "'{}' is not allowed",
                requested_program_name.to_string_lossy()
            )
        })?;

    ensure_whatever!(
        resolved_program_path == verified_program_path,
        "found symlink '{}' for '{}', but it resolves to '{}', not '{}'",
        allowed_program_path.to_string_lossy(),
        requested_program_name.to_string_lossy(),
        verified_program_path.to_string_lossy(),
        resolved_program_path.to_string_lossy(),
    );

    Ok((verified_program_path, requested_program_name))
}

// Holds required and blocked arguments for a program.
#[derive(Deserialize, Serialize, Debug)]
#[serde(rename_all = "kebab-case")]
struct ProgramSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    required_args: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    blocked_args: Option<HashSet<String>>,
}

// Verify the arguments for a program by reading from an optional config file:
// - required arguments must be found in the same positional order
// - blocked arguments must not be found in any position
fn verify_args(
    config_dir: impl AsRef<OsStr>,
    arg0: impl AsRef<OsStr>,
    args: &[String],
) -> Result<()> {
    let arg0 = arg0.as_ref();
    let mut arg0_toml = arg0.to_os_string();
    arg0_toml.push(".toml");
    let arg0_config: PathBuf = [config_dir.as_ref(), &arg0_toml].iter().collect();

    let config = fs::read_to_string(&arg0_config);
    if let Err(ref e) = config {
        if e.kind() == ErrorKind::NotFound {
            return Ok(());
        }
    }

    let config = config.with_whatever_context(|_| {
        format!("'{}' could not be read", arg0_config.to_string_lossy())
    })?;

    let settings: ProgramSettings =
        toml::from_str(config.as_str()).with_whatever_context(|_| {
            format!("'{}' could not be parsed", arg0_config.to_string_lossy())
        })?;

    let required_args = settings.required_args.unwrap_or_default();
    let mut missing_required_args: HashSet<String> =
        HashSet::from_iter(required_args.iter().cloned());
    required_args
        .iter()
        .zip(args.iter())
        .filter(|(a, b)| a == b)
        .for_each(|(a, _)| {
            missing_required_args.remove(a);
        });

    ensure_whatever!(
        missing_required_args.is_empty(),
        "missing required args for '{}': {}",
        arg0.to_string_lossy(),
        itertools::join(&missing_required_args, " "),
    );

    let blocked_args = settings.blocked_args.unwrap_or_default();
    let mut found_blocked_args = HashSet::new();
    args.iter()
        .filter(|a| blocked_args.contains(*a))
        .for_each(|a| {
            found_blocked_args.insert(a);
        });

    ensure_whatever!(
        found_blocked_args.is_empty(),
        "found blocked args for '{}': {}",
        arg0.to_string_lossy(),
        itertools::join(&found_blocked_args, " "),
    );

    Ok(())
}

// Wrapper function that either executes the requested program, if allowed, or returns an error.
fn run(
    args: Vec<String>,
    allowed_programs_dir: &str,
    config_dir: &str,
    allowed_paths: &str,
) -> Result<()> {
    let (program, args) = parse_args(args)?;
    let (program, arg0) = verify_program(allowed_programs_dir, allowed_paths, &program)?;
    verify_args(config_dir, &arg0, &args)?;

    let err = Command::new(&program).args(args).arg0(&arg0).exec();
    Err(Whatever::with_source(
        Box::new(err),
        format!("'{}' could not be executed", program.display()),
    ))
}

// *brush* should either silently execute the requested program if it is allowed, or else summarize
// any errors and return with exit code 127 so the calling application understands that it could
// not be executed.
fn main() -> ExitCode {
    let args = std::env::args().collect();
    if let Err(e) = run(args, ALLOWED_PROGRAMS_DIR, CONFIG_DIR, ALLOWED_PATHS) {
        eprintln!("{}", Report::from_error(e));
    }
    ExitCode::from(127)
}

#[cfg(test)]
mod test {
    use super::*;
    use std::fs::OpenOptions;
    use std::io::{BufWriter, Write};
    use std::os::unix::fs::{symlink, OpenOptionsExt};
    use std::path;

    use tempfile::TempDir;

    // Helper to join two paths where the second might be absolute.
    fn path_inside(tempdir: &TempDir, path: impl AsRef<Path>) -> PathBuf {
        let path = path.as_ref();
        let path = format!("./{}", path.display());
        path::absolute(tempdir.path().join(&path)).unwrap()
    }

    // Sets up a standard, usr-merged, sbin-merged filesystem, with a single coreutils binary.
    static STANDARD_LAYOUT: &str = r#"
            dir /usr/bin
            symlink /usr/sbin bin
            symlink /bin usr/bin
            symlink /sbin usr/sbin

            file /usr/bin/coreutils
            symlink /usr/bin/date coreutils
            symlink /usr/bin/false coreutils
            symlink /usr/bin/true coreutils
            symlink /usr/bin/rm coreutils

            dir /usr/libexec/brush/allowed-programs
            dir /usr/share/brush
        "#;

    // Creates the desired directory layout in the format above inside a temporary directory.
    fn populate_directory(tempdir: &TempDir, layout: &str) {
        for line in layout.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let mut line = line.split_whitespace();
            let cmd = line.next().unwrap();
            match cmd {
                "#" => continue,
                "dir" => {
                    let dir = line.next().unwrap();
                    let path = path_inside(tempdir, dir);
                    fs::create_dir_all(&path).unwrap();
                }
                "file" => {
                    let file = line.next().unwrap();
                    let path = path_inside(tempdir, file);
                    let mut buf = BufWriter::new(
                        OpenOptions::new()
                            .write(true)
                            .create(true)
                            .truncate(true)
                            .mode(0o755)
                            .open(path)
                            .unwrap(),
                    );
                    writeln!(buf, "#!{file}").unwrap();
                }
                "symlink" => {
                    let link_name = line.next().unwrap();
                    let link_name = path_inside(tempdir, link_name);
                    let mut link_target: PathBuf = line.next().unwrap().into();
                    if link_target.is_absolute() {
                        link_target = path_inside(tempdir, link_target);
                        link_target = pathdiff::diff_paths(&link_target, &link_name).unwrap();
                        link_target = link_target.strip_prefix("../").unwrap().to_path_buf();
                    }
                    symlink(link_target, link_name).unwrap();
                }
                _ => panic!("bad command {cmd}"),
            }
        }
    }

    #[test]
    fn test_resolve_symlink() {
        let tmpdir = TempDir::new().unwrap();
        populate_directory(&tmpdir, STANDARD_LAYOUT);

        let test_layout = format!(
            r#"
            # symlinks to `true` and `false` should resolve to `coreutils`
            symlink {ALLOWED_PROGRAMS_DIR}/true /usr/bin/true
            symlink {ALLOWED_PROGRAMS_DIR}/false /usr/bin/false
        "#
        );
        populate_directory(&tmpdir, &test_layout);

        macro_rules! tmp {
            ($path:expr) => {
                path_inside(&tmpdir, $path)
            };
        }

        assert_eq!(resolve_symlink(tmp!("/sbin")).unwrap(), tmp!("/usr/bin"));

        assert_eq!(
            resolve_symlink(tmp!(ALLOWED_PROGRAMS_DIR).join("true")).unwrap(),
            tmp!("/usr/bin/coreutils")
        );

        assert_eq!(
            resolve_symlink(tmp!(ALLOWED_PROGRAMS_DIR).join("false")).unwrap(),
            tmp!("/usr/bin/coreutils")
        );

        assert!(resolve_symlink(tmp!(ALLOWED_PROGRAMS_DIR).join("date")).is_err());
    }

    #[test]
    fn test_resolve_program() {
        let tmpdir = TempDir::new().unwrap();
        populate_directory(&tmpdir, STANDARD_LAYOUT);

        let test_layout = r#"
            dir /opt/cni/bin
            file /opt/cni/bin/cnitool
        "#
        .to_string();
        populate_directory(&tmpdir, &test_layout);

        macro_rules! tmp {
            ($path:expr) => {
                path_inside(&tmpdir, $path)
            };
        }

        let make_paths = |p: Vec<&str>| {
            let mut s = String::new();
            for x in p.iter() {
                s.push_str(&format!("{}:", tmp!(x).display()));
            }
            s
        };

        let paths = make_paths(vec!["/usr/sbin", "/usr/bin"]);
        assert_eq!(
            resolve_program("true", &paths).unwrap(),
            tmp!("/usr/bin/coreutils")
        );
        assert_eq!(
            resolve_program(tmp!("/sbin/true"), &paths).unwrap(),
            tmp!("/usr/bin/coreutils")
        );

        // `cnitool isn't in $PATH, so this is an error
        assert!(resolve_program("cnitool", &paths).is_err());

        let paths = make_paths(vec!["/opt/cni/bin"]);
        assert_eq!(
            resolve_program("cnitool", &paths).unwrap(),
            tmp!("/opt/cni/bin/cnitool")
        );
        assert_eq!(
            resolve_program(tmp!("/opt/cni/bin/cnitool"), &paths).unwrap(),
            tmp!("/opt/cni/bin/cnitool")
        );
        assert!(resolve_program("true", &paths).is_err());
    }

    #[test]
    fn test_verify_program() {
        let tmpdir = TempDir::new().unwrap();
        populate_directory(&tmpdir, STANDARD_LAYOUT);

        let test_layout = format!(
            r#"
            symlink /sbin/chroot coreutils

            file /usr/bin/cnitool

            dir /opt/cni/bin
            file /opt/cni/bin/cnitool

            dir /opt/csi/bin
            file /opt/csi/bin/s3-mountpoint

            symlink {ALLOWED_PROGRAMS_DIR}/true /usr/bin/true
            symlink {ALLOWED_PROGRAMS_DIR}/false /usr/bin/false
            symlink {ALLOWED_PROGRAMS_DIR}/chroot /sbin/chroot

            symlink {ALLOWED_PROGRAMS_DIR}/cnitool /opt/cni/bin/cnitool
            symlink {ALLOWED_PROGRAMS_DIR}/s3-mountpoint /opt/csi/bin/s3-mountpoint
        "#
        );
        populate_directory(&tmpdir, &test_layout);

        macro_rules! tmp {
            ($path:expr) => {
                path_inside(&tmpdir, $path)
            };
        }

        let make_paths = |p: Vec<&str>| {
            let mut s = String::new();
            for x in p.iter() {
                s.push_str(&format!("{}:", tmp!(x).display()));
            }
            s
        };

        let allowed_programs = tmp!(ALLOWED_PROGRAMS_DIR);
        let normal_paths = make_paths(vec!["/usr/sbin", "/usr/bin"]);

        // These all resolve to `coreutils` via symlinks.
        for allowed in ["true", "false", "chroot"] {
            assert_eq!(
                verify_program(&allowed_programs, &normal_paths, allowed).unwrap(),
                (tmp!("/usr/bin/coreutils"), filename(allowed).unwrap())
            );
        }

        // These also resolve to `coreutils`, but no symlink exists to allow them.
        for not_allowed in ["rm", "date"] {
            assert!(verify_program(&allowed_programs, &normal_paths, not_allowed).is_err());
        }

        let other_paths = make_paths(vec!["/opt/cni/bin", "/opt/csi/bin"]);

        // These programs are not in the normal paths, but should be located in other paths.
        for other in ["cnitool", "s3-mountpoint"] {
            assert!(verify_program(&allowed_programs, &normal_paths, other).is_err());
            assert!(verify_program(&allowed_programs, &other_paths, other).is_ok());
        }

        let all_paths = make_paths(vec!["/usr/bin", "/opt/cni/bin"]);

        // This succeeds because the absolute path for `cnitool` avoids the $PATH lookup.
        assert!(
            verify_program(&allowed_programs, &all_paths, tmp!("/opt/cni/bin/cnitool")).is_ok()
        );

        // This is an error because it finds `cnitool` in /usr/bin, but the allowed symlink points
        // to /opt/cni/bin.
        assert!(verify_program(&allowed_programs, &all_paths, "cnitool").is_err());
    }

    // Helper to build input to parse_args()
    fn args_from(s: &[&str]) -> Vec<String> {
        s.iter().map(|s| s.to_string()).collect()
    }

    // Helper to build return value from parse_args()
    fn cmd_from(s: &[&str]) -> (String, Vec<String>) {
        let mut s = s.iter();
        let program = s.next().unwrap().to_string();
        let args = s.map(|s| s.to_string()).collect();
        (program, args)
    }

    #[test]
    fn test_parse_args() {
        let good = args_from(&["sh", "-c", "qwerty --uiop asdf"]);
        let expected = cmd_from(&["qwerty", "--uiop", "asdf"]);
        assert_eq!(parse_args(good).unwrap(), expected);

        let bad = args_from(&["sh", "zxcvb"]);
        assert!(parse_args(bad).is_err());

        let bad = args_from(&["sh", "-c"]);
        assert!(parse_args(bad).is_err());

        let bad = args_from(&["sh", "-c", "querty", "--uiop", "asdf"]);
        assert!(parse_args(bad).is_err());
    }

    #[test]
    fn test_filename() {
        assert_eq!(filename("/usr/bin").unwrap(), "bin");
        assert!(filename("/usr/bin/..").is_err());
        assert!(filename("").is_err());
    }

    fn write_config(tempdir: &TempDir, config_dir: &str, arg0: impl AsRef<OsStr>, config: &str) {
        let config_file = path_inside(
            tempdir,
            format!("{config_dir}/{}.toml", arg0.as_ref().to_string_lossy()),
        );
        let mut buf = BufWriter::new(
            OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o644)
                .open(config_file)
                .unwrap(),
        );
        writeln!(buf, "{config}").unwrap();
    }

    #[test]
    fn test_verify_args() {
        let tmpdir = TempDir::new().unwrap();
        populate_directory(&tmpdir, STANDARD_LAYOUT);

        macro_rules! tmp {
            ($path:expr) => {
                path_inside(&tmpdir, $path)
            };
        }

        let arg0 = filename("foo").unwrap();
        let args = args_from(&["bar", "--debug"]);

        macro_rules! config {
            ($cfg:literal) => {
                write_config(&tmpdir, CONFIG_DIR, &arg0, $cfg)
            };
        }

        // With no config file, any arguments are allowed.
        assert!(verify_args(tmp!(CONFIG_DIR), &arg0, &args).is_ok());

        config!(
            r#"
            required-args = ["baz"]
        "#
        );

        // The first argument is "bar", not "baz".
        assert!(verify_args(tmp!(CONFIG_DIR), &arg0, &args).is_err());

        // No arguments are provided, and "bar" must be first.
        assert!(verify_args(tmp!(CONFIG_DIR), &arg0, &Vec::new()).is_err());

        config!(
            r#"
            required-args = ["bar"]
        "#
        );

        // The first argument is "bar", which is now required.
        assert!(verify_args(tmp!(CONFIG_DIR), &arg0, &args).is_ok());

        config!(
            r#"
            required-args = ["baz", "bar"]
        "#
        );

        // The first argument is "bar", but it must be the second argument.
        assert!(verify_args(tmp!(CONFIG_DIR), &arg0, &args).is_err());

        config!(
            r#"
            blocked-args = ["--debug"]
        "#
        );

        // The "--debug" argument must not be present.
        assert!(verify_args(tmp!(CONFIG_DIR), &arg0, &args).is_err());

        // The same command works without that argument.
        assert!(verify_args(tmp!(CONFIG_DIR), &arg0, &Vec::new()).is_ok());

        config!(
            r#"
            blocked-args = ["--trace"]
        "#
        );

        // Other arguments are allowed, just not "--trace".
        assert!(verify_args(tmp!(CONFIG_DIR), &arg0, &args).is_ok());
    }
}
