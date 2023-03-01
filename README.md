# System MQTT

Inspired by [system-bridge](https://github.com/timmo001/system-bridge), System MQTT is essentially the same thing but for a different audience.

System MQTT takes several statistics from the computer it is running on and then reports them to an MQTT broker. With that it also transmits the necessary discovery messages to that broker for Home Assistant to be made aware of the device.

At this point in time the following information is reported:

* CPU usage
* Memory usage
* Swap usage
* Filesystem usage
* Battery state
* Battery level

The advantage of system-mqtt is that it's light weight in comparison to system-bridge. Weighing in at under a Megabyte and a CPU usage so small I can't get it to show up under htop, system-mqtt is light enough to run on your Pi.

The downside of system-mqtt is that its meant more for power users. There's no system tray icon, no web interface, or really any UI at all. All of the configuration is done using a config folder under `/etc/system-mqtt.yaml`. It's easy enough to work with but not certainly not as pretty as system-bridge.

# Supported platforms

My main goal was for this to run on Linux, specifically Debian based distros since that's what I primarily use. In theory a port to Windows should require very minimal effort. Feel free to make a pull request if you wish to add such functionality. If you want some other package format like RPM, again, feel free to make a pull request and add such functionality.

# Adding more statistics

Want more statistics to be reported? I'm fine with that. Just make a pull request. My main requirements be that you run `cargo fmt`, avoid use of `unsafe`, keep the memory usage at runtime under a Megabyte, and keep the CPU usage unnoticed.

If for some reason your feature just can't be fit within those requirement, make the pull request anyway and we'll talk about it. I'm sure we can find a compromise.

# Dependencies

* Building `system-mqtt` requires the Rust toolchain including the `cargo` package manager. Default repositories may be out of date and fail to build, so it is recommended to install Rust using the installer at [Rustup](https://rustup.rs/). Remove any existing instances of `rustc` on your system, and install a fresh copy of Rust using the instructions on Rustup.rs.
* The Cargo helper command [cargo-deb](https://crates.io/crates/cargo-deb) is required to allow `cargo` to build a .deb package. Once Rust is installed, add `cargo-deb` with the command `cargo install cargo-deb`.
* `libdbus-1-dev` is required, but not installed by default on many Debian systems. `sudo apt install libdbus-1-dev` will install from the Debian or Ubuntu repositories.
* `libdbus-1-3` or similar is required, but is installed by default on most Debian and Ubuntu systems.

# Installation

I brewed this up in less than a day and have less than an hour of runtime with it, so I don't feel ready to publish proper releases or a fancy pre-packaged installer. This means you'll be installing it from source.

Step 1: [Setup MQTT with Home Assistant](https://www.home-assistant.io/integrations/mqtt/).

Step 2: Clone this repository.

Step 3: Verify all dependencies are installed.

Step 4: Run the command `cargo deb --install` from the cloned directory.

At this point you've installed `system-mqtt` as a debian package that can easily be removed. It will automatically be registered with systemd, but may require a manual start with `systemctl start system-mqtt`.

At this point the daemon is installed, but won't run if the mqtt broker is not running on the local system. You'll need to edit the configuration to let it know about the mqtt broker and its credentials.

# Configuration

The configuration file lives at `/etc/system-mqtt.yaml`.

If it does not appear when you first install system-mqtt, it will be created and populated when `system-mqtt` is run with default arguments.

Here is the default config with comments added explaining the configuration options:
```yaml
# The URL to the mqtt broker.
mqtt_server: "mqtt://localhost"

# If no authentication is needed to log into the mqtt broker, leave this be.
# If authentication is needed, set this to the user name. The password will
# be fetched from the OS keyring.
# To set that password, run `system-mqtt set-password` and an interactive
# prompt will ask you for the login password.
username: ~

# If unspecified, this will default to `keyring`, where it uses the system keyring for your password.
password_source: keyring

# Alternatively, you can use a "secret file" for your password. It's an unencrypted plaintext file with the password. You must set the file to be owned by the user running system-mqtt (typically root) and set the permissions so that only the user can access the file.
# This is derived from the security policies used by ssh and OpenPGP. It's a little less ideal than keyring so you should prefer keyring if you can use it.
# Here's an example of how you'd point to where that file is located:
# password_source: !secret_file /path/to/file

# The amount of time to wait between each report of the system statistics.
update_interval:
  secs: 30
  nanos: 0

# You can have multiple filesystem disk usages be reported.
# Each entry here should have its path be set to the root of the filesystem
# you wish to report the usage of, and the name is what name it will
# reported as to mqtt.
drives:
  - path: /
    name: root
```

Once you have adjusted the configuration as needed, run `systemctl reload system-mqtt` to restart the service with the new configuration.

Run `systemctl status system-mqtt` after to verify the configuration loaded and the daemon is running correctly.
