use anyhow::{bail, Context, Result};
use argh::FromArgs;
use mqtt_async_client::client::{Client as MqttClient, Publish};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    os::unix::prelude::MetadataExt,
    path::{Path, PathBuf},
    time::Duration,
};
use sysinfo::{CpuExt, DiskExt, System, SystemExt};
use tokio::{fs, signal, time};
use url::Url;

const KEYRING_SERVICE_NAME: &str = "system-mqtt";

#[derive(FromArgs)]
/// Push system statistics to an mqtt server.
struct Arguments {
    /// the configuration file we are to use.
    #[argh(option, default = "PathBuf::from(\"/etc/system-mqtt.yaml\")")]
    config_file: PathBuf,

    #[argh(subcommand)]
    command: SubCommand,
}

#[derive(FromArgs, PartialEq, Debug)]
#[argh(subcommand)]
enum SubCommand {
    Run(RunArguments),
    SetPassword(SetPasswordArguments),
}

#[derive(FromArgs, PartialEq, Debug)]
/// Run the daemon.
#[argh(subcommand, name = "run")]
struct RunArguments {}

#[derive(FromArgs, PartialEq, Debug)]
/// Set the password used to log into the mqtt client.
#[argh(subcommand, name = "set-password")]
struct SetPasswordArguments {}

#[derive(Serialize, Deserialize)]
struct DriveConfig {
    path: PathBuf,
    name: String,
}

#[derive(Serialize, Deserialize)]
enum PasswordSource {
    Keyring,
    SecretFile(PathBuf),
}

impl Default for PasswordSource {
    fn default() -> Self {
        Self::Keyring
    }
}

#[derive(Serialize, Deserialize)]
struct Config {
    /// The URL of the mqtt server.
    mqtt_server: Url,

    /// Set the username to connect to the mqtt server, if required.
    /// The password will be fetched from the OS keyring.
    username: Option<String>,

    /// Where the password for the MQTT server can be found.
    /// If a username is not specified, this field is ignored.
    /// If not specified, this field defaults to the keyring.
    #[serde(default)]
    password_source: PasswordSource,

    /// The interval to update at.
    update_interval: Duration,

    /// The names of drives, or the paths to where they are mounted.
    drives: Vec<DriveConfig>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mqtt_server: Url::parse("mqtt://localhost").expect("Failed to parse default URL."),
            username: None,
            password_source: PasswordSource::Keyring,
            update_interval: Duration::from_secs(30),
            drives: vec![DriveConfig {
                path: PathBuf::from("/"),
                name: String::from("root"),
            }],
        }
    }
}

#[tokio::main]
async fn main() {
    let arguments: Arguments = argh::from_env();

    match load_config(&arguments.config_file).await {
        Ok(config) => match arguments.command {
            SubCommand::Run(_arguments) => {
                mowl::init_with_level(log::LevelFilter::Info).expect("Failed to setup log.");

                while let Err(error) = application_trampoline(&config).await {
                    log::error!("Fatal error: {}", error);
                }
            }
            SubCommand::SetPassword(_arguments) => {
                if let Err(error) = set_password(config).await {
                    eprintln!("Fatal error: {}", error);
                }
            }
        },
        Err(error) => {
            eprintln!("Failed to load config file: {}", error);
        }
    }
}

async fn load_config(path: &Path) -> Result<Config> {
    if path.is_file() {
        // It's a readable file we can load.

        let config: Config = serde_yaml::from_str(&fs::read_to_string(path).await?)
            .context("Failed to deserialize config file.")?;

        Ok(config)
    } else {
        log::info!("No config file present. A default one will be written.");
        // Doesn't exist yet. We'll create it.
        let config = Config::default();

        // Write it to a file for next time we load.
        fs::write(path, serde_yaml::to_string(&config)?).await?;

        Ok(config)
    }
}

async fn set_password(config: Config) -> Result<()> {
    if let Some(username) = config.username {
        let password = rpassword::prompt_password("Password: ")
            .context("Failed to read password from TTY.")?;

        let keyring = keyring::Entry::new(KEYRING_SERVICE_NAME, &username)
            .context("Failed to find password entry in keyring.")?;
        keyring.set_password(&password).context("Keyring error.")?;

        Ok(())
    } else {
        bail!("You must set the username for login with the mqtt server before you can set the user's password")
    }
}

async fn application_trampoline(config: &Config) -> Result<()> {
    log::info!("Application start.");

    let mut client_builder = MqttClient::builder();
    client_builder.set_url_string(config.mqtt_server.as_str())?;

    // If credentials are provided, use them.
    if let Some(username) = &config.username {
        // TODO make TLS mandatory when using this.

        let password = match &config.password_source {
            PasswordSource::Keyring => {
                log::info!("Using system keyring for MQTT password source.");
                let keyring = keyring::Entry::new(KEYRING_SERVICE_NAME, username)
                    .context("Failed to find password entry in keyring.")?;
                keyring
                    .get_password()
                    .context("Failed to get password from keyring. If you have not yet set the password, run `system-mqtt set-password`.")?
            }
            PasswordSource::SecretFile(file_path) => {
                log::info!("Using hidden file for MQTT password source.");
                let metadata = file_path
                    .metadata()
                    .context("Failed to get password file metadata.")?;

                // It's not even an encrypted file, so we need to keep the permission settings pretty tight.
                // The only time I can really enforce that is when reading the password.
                if metadata.mode() == 0o600 {
                    if metadata.uid() == users::get_current_uid() {
                        if metadata.gid() == users::get_current_gid() {
                            fs::read_to_string(file_path)
                                .await
                                .context("Failed to read password file.")?
                        } else {
                            bail!("Password file must be owned by the current group.");
                        }
                    } else {
                        bail!("Password file must be owned by the current user.");
                    }
                } else {
                    bail!("Permission bits for password file must be set to 0o600 (only owner can read and write)");
                }
            }
        };

        client_builder.set_username(Some(username.into()));
        client_builder.set_password(Some(password.into()));
    }

    let mut client = client_builder.build()?;
    client
        .connect()
        .await
        .context("Failed to connect to MQTT server.")?;

    let manager = battery::Manager::new().context("Failed to initalize battery monitoring.")?;

    let mut system = System::new_all();

    let hostname = system
        .host_name()
        .context("Could not get system hostname.")?;

    let mut home_assistant = HomeAssistant {
        client,
        hostname,
        registered_topics: HashSet::new(),
    };

    // Register the various sensor topics and include the details about that sensor

    //    TODO - create a new register_topic to register binary_sensor so we can make availability a real binary sensor. In the
    //    meantime, create it as a normal analog sensor with two values, and a template can be used to make it a binary.

    home_assistant
        .register_topic(
            "sensor",
            None,
            Some(""),
            "available",
            None,
            Some("mdi:check-network-outline"),
        )
        .await
        .context("Failed to register availability topic.")?;
    home_assistant
        .register_topic(
            "sensor",
            None,
            Some(""),
            "uptime",
            Some("days"),
            Some("mdi:timer-sand"),
        )
        .await
        .context("Failed to register uptime topic.")?;
    home_assistant
        .register_topic(
            "sensor",
            None,
            Some("measurement"),
            "cpu",
            Some("%"),
            Some("mdi:gauge"),
        )
        .await
        .context("Failed to register CPU usage topic.")?;
    home_assistant
        .register_topic(
            "sensor",
            None,
            Some("measurement"),
            "memory",
            Some("%"),
            Some("mdi:gauge"),
        )
        .await
        .context("Failed to register memory usage topic.")?;
    home_assistant
        .register_topic(
            "sensor",
            None,
            Some("measurement"),
            "swap",
            Some("%"),
            Some("mdi:gauge"),
        )
        .await
        .context("Failed to register swap usage topic.")?;
    home_assistant
        .register_topic(
            "sensor",
            Some("battery"),
            Some("measurement"),
            "battery_level",
            Some("%"),
            Some("mdi:battery"),
        )
        .await
        .context("Failed to register battery level topic.")?;
    home_assistant
        .register_topic(
            "sensor",
            None,
            Some(""),
            "battery_state",
            None,
            Some("mdi:battery"),
        )
        .await
        .context("Failed to register battery state topic.")?;

    // Register the sensors for filesystems
    for drive in &config.drives {
        home_assistant
            .register_topic(
                "sensor",
                None,
                Some("total"),
                &drive.name,
                Some("%"),
                Some("mdi:folder"),
            )
            .await
            .context("Failed to register a filesystem topic.")?;
    }

    home_assistant.set_available(true).await?;

    let result = availability_trampoline(&home_assistant, &mut system, config, manager).await;

    if let Err(error) = home_assistant.set_available(false).await {
        // I don't want this error hiding whatever happened in the main loop.
        log::error!("Error while disconnecting from home assistant: {:?}", error);
    }

    result?;

    home_assistant.disconnect().await?;

    Ok(())
}

async fn availability_trampoline(
    home_assistant: &HomeAssistant,
    system: &mut System,
    config: &Config,
    manager: battery::Manager,
) -> Result<()> {
    let drive_list: HashMap<PathBuf, String> = config
        .drives
        .iter()
        .map(|drive_config| (drive_config.path.clone(), drive_config.name.clone()))
        .collect();

    loop {
        system.refresh_disks();
        system.refresh_memory();
        system.refresh_cpu();

        tokio::select! {
            _ = time::sleep(config.update_interval) => {
                // Report uptime.
                let uptime = system.uptime() as f32 / 60.0 / 60.0 / 24.0; // Convert from seconds to days.
                home_assistant.publish("uptime", format!("{}", uptime)).await;

                // Report CPU usage.
                let cpu_usage = system.cpus().iter().map(|cpu| cpu.cpu_usage()).sum::<f32>() / system.cpus().len() as f32;
                home_assistant.publish("cpu", (cpu_usage.clamp(0.0, 1.0) * 100.0).to_string()).await;

                // Report memory usage.
                let memory_percentile = (system.total_memory() - system.available_memory()) as f64 / system.total_memory() as f64;
                home_assistant.publish("memory", (memory_percentile.clamp(0.0, 1.0)* 100.0).to_string()).await;

                // Report swap usage.
                let swap_percentile = system.used_swap() as f64 / system.free_swap() as f64;
                home_assistant.publish("swap", (swap_percentile.clamp(0.0, 1.0) * 100.0).to_string()).await;

                // Report filesystem usage.
                for drive in system.disks() {
                    if let Some(drive_name) = drive_list.get(drive.mount_point()) {
                        let drive_percentile = (drive.total_space() - drive.available_space()) as f64 / drive.total_space() as f64;

                        home_assistant.publish(drive_name, (drive_percentile.clamp(0.0, 1.0) * 100.0).to_string()).await;
                    }
                }

                // TODO we should probably combine the battery charges, but for now we're just going to use the first detected battery.
                if let Some(battery) = manager.batteries().context("Failed to read battery info.")?.flatten().next() {
                    use battery::State;

                    let battery_state = match battery.state() {
                        State::Charging => "charging",
                        State::Discharging => "discharging",
                        State::Empty => "empty",
                        State::Full => "full",
                        _ => "unknown",
                    };

                    home_assistant.publish("battery_state", battery_state.to_string()).await;

                    let battery_full = battery.energy_full();
                    let battery_power = battery.energy();
                    let battery_level = battery_power / battery_full;

                    home_assistant.publish("battery_level", format!("{:03}", battery_level.value)).await;
                }
            }
            _ = signal::ctrl_c() => {
                log::info!("Terminate signal has been received.");
                break;
            }
        }
    }

    Ok(())
}

pub struct HomeAssistant {
    client: MqttClient,
    hostname: String,
    registered_topics: HashSet<String>,
}

impl HomeAssistant {
    pub async fn set_available(&self, available: bool) -> Result<()> {
        self.client
            .publish(
                Publish::new(
                    format!("system-mqtt/{}/availability", self.hostname),
                    if available { "online" } else { "offline" }.into(),
                )
                .set_retain(true),
            )
            .await
            .context("Failed to publish availability topic.")
    }

    pub async fn register_topic(
        &mut self,
        topic_class: &str,
        device_class: Option<&str>,
        state_class: Option<&str>,
        topic_name: &str,
        unit_of_measurement: Option<&str>,
        icon: Option<&str>,
    ) -> Result<()> {
        log::info!("Registering topic `{}`.", topic_name);

        #[derive(Serialize)]
        struct TopicConfig {
            name: String,

            #[serde(skip_serializing_if = "Option::is_none")]
            device_class: Option<String>,
            state_class: Option<String>,
            state_topic: String,
            unit_of_measurement: Option<String>,
            icon: Option<String>,
        }

        let message = serde_json::ser::to_string(&TopicConfig {
            name: format!("{}-{}", self.hostname, topic_name),
            device_class: device_class.map(str::to_string),
            state_class: state_class.map(str::to_string),
            state_topic: format!("system-mqtt/{}/{}", self.hostname, topic_name),
            unit_of_measurement: unit_of_measurement.map(str::to_string),
            icon: icon.map(str::to_string),
        })
        .context("Failed to serialize topic information.")?;
        let mut publish = Publish::new(
            format!(
                "homeassistant/{}/system-mqtt-{}/{}/config",
                topic_class, self.hostname, topic_name
            ),
            message.into(),
        );
        publish.set_retain(true);
        self.client
            .publish(&publish)
            .await
            .context("Failed to publish topic to MQTT server.")?;

        self.registered_topics.insert(topic_name.to_string());

        Ok(())
    }

    pub async fn publish(&self, topic_name: &str, value: String) {
        log::debug!("PUBLISH `{}` TO `{}`", value, topic_name);

        if self.registered_topics.contains(topic_name) {
            let mut publish = Publish::new(
                format!("system-mqtt/{}/{}", self.hostname, topic_name),
                value.into(),
            );
            publish.set_retain(false);

            if let Err(error) = self.client.publish(&publish).await {
                log::error!("Failed to publish topic `{}`: {:?}", topic_name, error);
            }
        } else {
            log::error!(
                "Attempt to publish topic `{}`, which was never registered with Home Assistant.",
                topic_name
            );
        }
    }

    pub async fn disconnect(mut self) -> Result<()> {
        self.set_available(false).await?;
        self.client.disconnect().await?;

        Ok(())
    }
}
