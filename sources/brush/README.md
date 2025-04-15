# brush

Current version: 0.1.0

## Introduction

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

## Colophon

This text was generated using [cargo-readme](https://crates.io/crates/cargo-readme), and includes the rustdoc from `src/main.rs`.
