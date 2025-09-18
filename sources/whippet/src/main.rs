/*!
# Introduction

*whippet* is Bottlerocket's implementation of a launcher for `dbus-broker`. It
implements the minimal set of features to configure and start the `dbus-broker`
which include:

- Policy configuration
- Systemd socket activation

## Writing D-Bus policies for whippet
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

## Policy drop-in files

*whippet* supports reading additional policy files from `/usr/share/whippet`
which will be loaded in lexicographic order. Rules loaded last can be used to
override rules defined by a previously loaded policy.
*/

#[macro_use]
extern crate log;

mod broker;
mod child;
mod dbus_policy;
mod error;
mod policy;
mod socket;

use crate::dbus_policy::DbusPolicy;
use crate::error::Result;
use crate::policy::{PasswdUsernameResolver, Policy};
use argh::FromArgs;
use broker::BrokerManager;
use log::LevelFilter;
use simplelog::{Config as LogConfig, SimpleLogger};
use snafu::ResultExt;

/// D-Bus launcher replacement for Bottlerocket
#[derive(FromArgs)]
struct Args {
    /// path to TOML configuration file
    #[argh(
        option,
        long = "config-file",
        default = "String::from(\"/usr/share/whippet/system.toml\")"
    )]
    config_file: String,

    /// path to dbus-broker binary
    #[argh(
        option,
        long = "broker-path",
        default = "String::from(\"/usr/bin/dbus-broker\")"
    )]
    broker_path: String,

    /// path to policy directory
    #[argh(
        option,
        long = "policy-dir",
        default = "policy::DBUS_POLICY_DROPINS_PATH.to_string()"
    )]
    policy_dir: String,

    /// log level (error, warn, info, debug, trace)
    #[argh(option, long = "log-level", default = "LevelFilter::Info")]
    log_level: LevelFilter,
}

#[tokio::main]
#[snafu::report]
async fn main() -> Result<()> {
    let args: Args = argh::from_env();

    SimpleLogger::init(args.log_level, LogConfig::default()).context(error::LoggerSnafu)?;

    info!("Starting whippet D-Bus launcher");
    debug!("Config file: {}", args.config_file);
    debug!("Broker path: {}", args.broker_path);

    let username_resolver = PasswdUsernameResolver::default();
    let mut policy = Policy::new(&args.config_file, &args.policy_dir, username_resolver)?;
    policy.prepare()?;
    policy.optimize();

    let dbus_policy: DbusPolicy = policy.try_into()?;

    info!("Starting dbus-broker: {}", args.broker_path);
    let broker = BrokerManager::new(&args.broker_path, dbus_policy)?;
    broker.run().await?;

    Ok(())
}
