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

# Installation

I brewed this up in less than a day and have less than an hour of runtime with it, so I don't feel ready to publish proper releases or a fancy pre-packaged installer. This means you'll be installing it from source.

Step 1: [Setup MQTT with Home Assistant](https://www.home-assistant.io/integrations/mqtt/).

Step 2: Clone this repository.

Step 3: Make sure you have [cargo](https://doc.rust-lang.org/cargo/getting-started/installation.html) installed.

Step 4: Install [cargo deb](https://crates.io/crates/cargo-deb).

Step 5: Run the command `cargo deb --install`.

At this point you've installed system-mqtt as a debian package that can easily be removed.

At this point the daemon is installed, but won't run if the mqtt broker is not running on the local system. You'll need to edit the configuration to let it know about the mqtt broker and its credentials.

# Configuration

The configuration file lives under `/etc/system-mqtt.yaml`.
If it does not when you install system-mqtt, it will be created and populated with default arguments.

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