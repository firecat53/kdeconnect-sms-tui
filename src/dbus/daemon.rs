use std::collections::HashMap;
use std::time::Duration;

use color_eyre::Result;
use tokio::time::timeout;
use tracing::{debug, info, warn};
use zbus::Connection;

use crate::models::device::Device;

const KDECONNECT_SERVICE: &str = "org.kde.kdeconnect";
const DAEMON_PATH: &str = "/modules/kdeconnect";
const DAEMON_INTERFACE: &str = "org.kde.kdeconnect.daemon";
const DEVICE_INTERFACE: &str = "org.kde.kdeconnect.device";

/// Timeout for D-Bus method calls.
const DBUS_TIMEOUT: Duration = Duration::from_secs(5);

/// Client for the kdeconnect daemon D-Bus interface.
pub struct DaemonClient {
    connection: Connection,
}

impl DaemonClient {
    pub async fn new() -> Result<Self> {
        let connection = Connection::session().await?;
        info!("Connected to session D-Bus");
        Ok(Self { connection })
    }

    /// Get list of device IDs.
    pub async fn device_ids(&self, only_reachable: bool, only_paired: bool) -> Result<Vec<String>> {
        let reply: Vec<String> = timeout(
            DBUS_TIMEOUT,
            self.connection.call_method(
                Some(KDECONNECT_SERVICE),
                DAEMON_PATH,
                Some(DAEMON_INTERFACE),
                "devices",
                &(only_reachable, only_paired),
            ),
        )
        .await
        .map_err(|_| color_eyre::eyre::eyre!("D-Bus call timed out: devices"))??
        .body()
        .deserialize()?;
        debug!("Got {} device IDs", reply.len());
        Ok(reply)
    }

    /// Get map of device ID → device name.
    pub async fn device_names(
        &self,
        only_reachable: bool,
        only_paired: bool,
    ) -> Result<HashMap<String, String>> {
        let reply: HashMap<String, String> = timeout(
            DBUS_TIMEOUT,
            self.connection.call_method(
                Some(KDECONNECT_SERVICE),
                DAEMON_PATH,
                Some(DAEMON_INTERFACE),
                "deviceNames",
                &(only_reachable, only_paired),
            ),
        )
        .await
        .map_err(|_| color_eyre::eyre::eyre!("D-Bus call timed out: deviceNames"))??
        .body()
        .deserialize()?;
        Ok(reply)
    }

    /// Get a device ID by name.
    pub async fn device_id_by_name(&self, name: &str) -> Result<String> {
        let reply: String = timeout(
            DBUS_TIMEOUT,
            self.connection.call_method(
                Some(KDECONNECT_SERVICE),
                DAEMON_PATH,
                Some(DAEMON_INTERFACE),
                "deviceIdByName",
                &name,
            ),
        )
        .await
        .map_err(|_| color_eyre::eyre::eyre!("D-Bus call timed out: deviceIdByName"))??
        .body()
        .deserialize()?;
        Ok(reply)
    }

    /// Get a property from a device's D-Bus interface.
    async fn get_device_property<T>(&self, device_id: &str, property: &str) -> Result<T>
    where
        T: TryFrom<zbus::zvariant::OwnedValue>,
        T::Error: Into<zbus::Error>,
    {
        let path = format!("/modules/kdeconnect/devices/{}", device_id);
        let proxy = timeout(
            DBUS_TIMEOUT,
            zbus::fdo::PropertiesProxy::builder(&self.connection)
                .destination(KDECONNECT_SERVICE)?
                .path(path.as_str())?
                .build(),
        )
        .await
        .map_err(|_| color_eyre::eyre::eyre!("D-Bus call timed out: build proxy"))??;
        let iface_name: zbus::names::InterfaceName<'_> = DEVICE_INTERFACE.try_into()?;
        let val = timeout(DBUS_TIMEOUT, proxy.get(iface_name, property))
            .await
            .map_err(|_| {
                color_eyre::eyre::eyre!("D-Bus call timed out: get property {}", property)
            })??;
        Ok(val.try_into().map_err(Into::into)?)
    }

    /// Discover all paired devices with their status.
    pub async fn discover_devices(&self) -> Result<Vec<Device>> {
        // Get all paired devices (reachable or not)
        let names = self.device_names(false, true).await?;
        let mut devices = Vec::new();

        for (id, name) in names {
            let reachable = self
                .get_device_property::<bool>(&id, "isReachable")
                .await
                .unwrap_or_else(|e| {
                    warn!("Failed to get reachable status for {}: {}", id, e);
                    false
                });

            devices.push(Device {
                id,
                name,
                reachable,
                paired: true,
            });
        }

        devices.sort_by(|a, b| {
            // Available devices first, then by name
            b.is_available()
                .cmp(&a.is_available())
                .then(a.name.cmp(&b.name))
        });

        info!("Discovered {} paired devices", devices.len());
        Ok(devices)
    }

    /// Find device by ID or name, or pick the first available.
    pub async fn resolve_device(
        &self,
        device_id: Option<&str>,
        device_name: Option<&str>,
    ) -> Result<Option<Device>> {
        if let Some(name) = device_name {
            let id = self.device_id_by_name(name).await?;
            if !id.is_empty() {
                let devices = self.discover_devices().await?;
                return Ok(devices.into_iter().find(|d| d.id == id));
            }
        }

        if let Some(id) = device_id {
            let devices = self.discover_devices().await?;
            return Ok(devices.into_iter().find(|d| d.id == id));
        }

        // Default: first available device
        let devices = self.discover_devices().await?;
        Ok(devices.into_iter().find(|d| d.is_available()))
    }

    pub fn connection(&self) -> &Connection {
        &self.connection
    }
}
