# How to build the Bottlerocket core kit

If you'd like to build your own copy of the core kit for local development, follow these steps.

## Dependencies
#### System Requirements
The build process artifacts and resulting images can consume in excess of 80GB in the local directory.
The build process is also fairly demanding on your CPU, since we build all included software from scratch. This is only done the first time. Package builds are cached, and only changes are built afterward.
The build scales well to 32+ cores.
The first time you build, the fastest machines can take about 20 minutes while slower machines with only a couple cores can take 3-4 hours.
#### Linux
The build system requires certain operating system packages to be installed.
Ensure the following packages are installed:
##### Ubuntu
```shell
apt install build-essential openssl libssl-dev pkg-config liblz4-tool cmake clang libclang-dev
```
##### Fedora
```shell
dnf install make automake gcc openssl openssl-devel pkg-config lz4 perl-FindBin perl-lib cmake clang clang-tools-extra
```

#### Rust
The build system is based on the Rust language.
We recommend you install the latest stable Rust using [rustup](https://rustup.rs/), either from the official site or your development host's package manager.
Rust 1.51.0 or higher is required.
To organize build tasks, we use [cargo-make](https://sagiegurari.github.io/cargo-make/).
To get it, run:
```shell
cargo install cargo-make
```

#### Docker
You'll need to have Docker installed and running, with your user account added to the `docker` group.
Docker's [post-installation steps for Linux](https://docs.docker.com/install/linux/linux-postinstall/) will walk you through that.

## Build the core kit

Building the core kit can be done by using the makefile targets.
```
make ARCH=<architecture>
```

## Publish the Kit
After the kit has been built you can then publish the kit image to your private registry. This will allow you to consume it to build and test a variant.

### Use a private registry for development
It is recommended that you have some form of protected container registry to use for testing.
For testing purposes you can either utilize mutable tags to allow overriding of multiple versions of a core kit as you test, or you can use immutable tags and continuously bump the core kit version via the `Twoliter.toml`. 

### Configure Infra.toml
An `Infra.toml` file needs to be created and should have a definition of your vendor (container registry) in order to publish the kits you build. To do so make sure that the `Infra.toml` has the below.
```
[vendor.<vendor-name>]
registry = "####.dkr.ecr.us-west-2.amazonaws.com"
```
After the kit has been built locally, the kit can be published to the provided vendor in `Infra.toml`. To do this, you will need docker credentials with ECR access. You can do this with,
```
aws ecr get-login-password --region us-west-2 | docker login --username AWS --password-stdin ####.dkr.ecr.us-west-2.amazonaws.com
```

Finally, publishing the core kit images can be handled by the makefile target.
```
make publish VENDOR=<vendor-name>
```
At this point, there should be a core kit image in your private registry which can be consumed when building a variant to test and validate.

## Consuming the published kit image
This section will cover building a variant to test a build of the core kit as done above. Please note this section does not cover the complete complexity of testing a change to Bottlerocket. For this see the [BUILDING](https://github.com/bottlerocket-os/bottlerocket/blob/develop/BUILDING.md) section in the [Bottlerocket](https://github.com/bottlerocket-os/bottlerocket/) repository.

### Configure Twoliter.toml
To consume a private copy of the Bottlerocket core kit with your changes built into it, you need to define the vendor that points to your container registry in `Twoliter.toml` and adjust the core kit dependency:
```
[vendor.my-vendor]
registry = "####.dkr.ecr.us-west-2.amazonaws.com"
[[kit]]
name = "bottlerocket-core-kit" # Name of your ECR repo
version = "2.x.y" # your version tag you want to test
vendor = "my-vendor"
```
Any time you change the vendor or version of the kit above you need to run `twoliter update` to update the `Twoliter.lock`
```
./tools/twoliter/twoliter update
```
