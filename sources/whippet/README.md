# whippet

Current version: 0.1.0

## Introduction

*whippet* is Bottlerocket's implementation of a launcher for `dbus-broker`. It
implements the minimal set of features to configure and start the `dbus-broker`
which include:

- Policy configuration
- Systemd socket activation

### Writing D-Bus policies for whippet
*whippet* uses TOML to define Dbus policies. This is an example of how to
define rules for the default context:

```toml
[policy.default]
rules = [
    { allow = true, user = "*" },             # This is a connect user rule
    { allow = true, own = "*" },              # This is an own rule
    { allow = true, send_destination = "*" }, # This is a send rule
    { allow = true, receive_sender = "*"},    # This is a receive rule
]
```

The rules in the default context will be included in the list of rules that
apply to all the users in the policy.

*whippet* supports user-specific rules as maps where the keys are either the
user names or user ids of the target users and the values are the list of
rules. This is an example to define rules for the users "pixie" and `1001`:

```toml
# A rule for user Pixie
[policy.user.pixie]
rules = [
    { allow = false, own = "*" }
]

# A rule for UID 1001
[policy.user."1001"]
rules = [
    { allow = false, own = "*" }
]
```

*whippet* supports all the fields supported in the original dbus-daemon policy
format, except:

- `mandatory`, `at_console`, and `group` contexts
- The `eavesdrop` field in rules
- The `own_prefix` field in own rules
- The `send_error`, `send_destination_prefix`, and `send_requested_reply`
  fields in send rules
- The `receive_error` and `receive_requested_reply` fields in receive rules

### Policy drop-in files

*whippet* supports reading additional policy files from `/usr/share/whippet`
which will be loaded in lexicographic order. Rules loaded last can be used to
override rules defined by a previously loaded policy.

## Colophon

This text was generated using [cargo-readme](https://crates.io/crates/cargo-readme), and includes the rustdoc from `src/main.rs`.
