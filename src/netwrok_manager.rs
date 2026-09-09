use crate::{
    dbus::{
        AccessPointProxyProxy, ActiveConnectionProxy, DeviceProxy, NetworkManagerProxy,
        NetworkManagerSettingsProxy, SettingsConnectionProxy, WirelessDeviceProxy,
    },
    structure::{AccessPoint, AppState, BSSID, PrivateData},
    utils,
};
use anyhow::{self, Context};
use futures_util::{StreamExt, try_join};
use std::collections::HashMap;
use zbus::{
    Connection, Proxy, blocking,
    zvariant::{Array, ObjectPath, OwnedObjectPath, OwnedValue, Value},
};

#[derive(Debug)]
// This stores blocking and must be converted into an async function.
// Converting to async is cheap since both are just thin wrappers around the same connection.
pub struct NetworkManagerDbusProxy {
    pub conn: zbus::Connection, //clone of con is cheap
    pub wifi_proxy: WirelessDeviceProxy<'static>,
    // pub property_proxy: zbus::fdo::PropertiesProxy<'static>,
    pub device_path: zbus::zvariant::OwnedObjectPath,
}

// Using async so, the scanning animation can be shown while, zbus requests to networkmanager for rescan.
pub async fn resolve_wireless_device(iface: &str) -> anyhow::Result<WirelessDeviceProxy> {
    let conn = Connection::system().await?;
    let network_manager = NetworkManagerProxy::new(&conn).await?;

    for device_path in network_manager.get_devices().await? {
        let device = DeviceProxy::new(&conn, device_path.clone()).await?;

        if device.device_type().await? == 2 && device.interface().await? == iface {
            return Ok(WirelessDeviceProxy::new(&conn, device_path).await?);
        }
    }
    anyhow::bail!("Failed to create dbus proxies")
}

pub async fn trigger_rescan(wireless: &WirelessDeviceProxy<'static>) -> anyhow::Result<()> {
    let mut stream = wireless.receive_last_scan_changed().await;

    // trigger rescan
    wireless.request_scan(&HashMap::new());

    while let Some(_val) = stream.next().await {
        return Ok(());
    }

    anyhow::bail!("Expected Some, got None")
}

pub async fn fetch_aps(wireless: &WirelessDeviceProxy<'_>) -> anyhow::Result<Vec<AccessPoint>> {
    let ap_paths = wireless.get_all_access_points().await?;
    let mut access_points = Vec::with_capacity(ap_paths.len());

    for ap_path in ap_paths {
        let ap = AccessPointProxyProxy::new(wireless.inner().connection(), &ap_path).await?;
        let (wpa_flags, rsn_flags, ssid, strength, bssid, frequency, auth_flag) = try_join!(
            ap.wpa_flags(),
            ap.rsn_flags(),
            ap.ssid(),
            ap.strength(),
            ap.hw_address(),
            ap.frequency(),
            ap.flags(),
        )?;

        // https://people.freedesktop.org/~lkundrak/nm-dbus-api/nm-dbus-types.html#NM80211ApSecurityFlags

        // skip enterprise / 802.1X networks.
        if (wpa_flags & 512 == 512) || (rsn_flags & 512 == 512) {
            //flag 256 is NM_802_11_AP_SEC_KEY_MGMT_802_1X
            continue;
        }

        access_points.push(AccessPoint {
            bssid,
            frequency,
            is_protected: auth_flag & 1 == 1, //https://people.freedesktop.org/~lkundrak/nm-dbus-api/nm-dbus-types.html#NM80211ApFlags
            signal_strength: strength,
            ssid: String::from_utf8_lossy(&ssid).to_string(),
            setting_path: None,
        });
    }

    // Looking into all the saved connections.
    let settings_proxy = NetworkManagerSettingsProxy::new(wireless.inner().connection()).await?;
    let connection_paths = settings_proxy.list_connections().await?;

    for connection_path in connection_paths {
        let connection_proxy =
            SettingsConnectionProxy::new(wireless.inner().connection(), &connection_path).await?;
        let settings = connection_proxy.get_settings().await?;

        let Some(raw_bssid) = settings
            .get("802-11-wireless")
            .and_then(|x| x.get("bssid"))
            .and_then(|v| v.downcast_ref::<Array>().ok())
            .map(|v| v.to_vec())
        else {
            continue;
        };

        let bssid = raw_bssid
            .iter()
            .filter_map(|x| x.downcast_ref::<u8>().ok())
            .map(|v| format!("{:02X}", v))
            .collect::<Vec<String>>()
            .join(":");

        if let Some(access_point) = access_points.iter_mut().find(|ap| ap.bssid == bssid) {
            access_point.setting_path = Some(connection_path);
        }
    }

    Ok(access_points)
}

pub async fn get_active_ap(
    wireless: &WirelessDeviceProxy<'_>,
) -> anyhow::Result<Option<(BSSID, OwnedObjectPath)>> {
    let connection = wireless.inner().connection();
    let device_proxy = DeviceProxy::new(connection, wireless.inner().path()).await?;

    let active_path = device_proxy.active_connection().await?;
    if active_path.as_str() == "/" {
        return Ok(None);
    }

    let active_connection = ActiveConnectionProxy::new(connection, &active_path).await?;
    let settings_path = active_connection.connection().await?;

    let ap_path = active_connection.specific_object().await?;
    let ap = AccessPointProxyProxy::new(connection, &ap_path).await?;

    let bssid = ap.hw_address().await?;

    Ok(Some((bssid, settings_path)))
}

pub async fn connect_pre_existing_access_point(
    wireless: &WirelessDeviceProxy<'_>,
    access_point: &AccessPoint,
) -> anyhow::Result<OwnedObjectPath> {
    let connection = wireless.inner().connection();
    let nm_proxy = NetworkManagerProxy::new(connection).await?;
    let setting_path = access_point
        .setting_path
        .as_ref()
        .expect("setting path is required");

    let active_ap_path: OwnedObjectPath = nm_proxy
        .activate_connection(
            setting_path.as_ref(),
            wireless.inner().path().as_ref(),
            ObjectPath::try_from("/")?,
        )
        .await?;

    let active_conn_proxy = ActiveConnectionProxy::new(connection, active_ap_path).await?;
    let settings = active_conn_proxy.connection().await?;

    Ok(settings)
}

// todo!(): Remove the duct tape and handle hidden Wi-Fi properly.
pub async fn create_and_connect_access_point(
    access_point: &AccessPoint,
    wireless: &WirelessDeviceProxy<'_>,
    password: Option<String>,
    hidden: Option<String>,
) -> anyhow::Result<OwnedObjectPath> {
    let connection = wireless.inner().connection();
    let nm_proxy = NetworkManagerProxy::new(connection).await?;

    let mut connection_settings = HashMap::new();

    let mut con_section: HashMap<&str, Value<'_>> = HashMap::new();
    con_section.insert("type", Value::from("802-11-wireless"));
    con_section.insert("uuid", Value::from(utils::generate_uuid()));
    con_section.insert("id", Value::from(&access_point.ssid));
    connection_settings.insert("connection", con_section);

    let mut wireless_section = HashMap::new();
    wireless_section.insert("ssid", Value::from(access_point.ssid.as_bytes()));
    wireless_section.insert("hidden", hidden.is_some().into());
    wireless_section.insert("mode", Value::from("infrastructure"));

    if hidden.is_none() {
        wireless_section.insert("bssid", Value::from(bssid_to_bytes(&access_point.bssid)));
    }

    connection_settings.insert("802-11-wireless", wireless_section);

    if access_point.is_protected {
        let mut s_wifi_sec = HashMap::new();
        s_wifi_sec.insert("key-mgmt", Value::from("wpa-psk"));
        s_wifi_sec.insert(
            "psk",
            Value::from(
                password.expect("If access point is set to protected, password should exist"),
            ),
        );

        connection_settings.insert("802-11-wireless-security", s_wifi_sec);
    }

    // maybe should switch to AddAndActivateConnection2
    let (sys, _) = nm_proxy
        .add_and_activate_connection(
            connection_settings,
            wireless.inner().path().as_ref(),
            ObjectPath::try_from("/")?,
        )
        .await?;

    Ok(sys)
}

pub async fn network_state<'a>(wireless: &WirelessDeviceProxy<'_>) -> anyhow::Result<u32> {
    let connection = wireless.inner().connection();
    let device_proxy = DeviceProxy::new(connection, wireless.inner().path()).await?;

    let mut state_stream = device_proxy.receive_state_changed().await?;

    while let Some(state) = state_stream.next().await {
        let state = state.args()?;
        if state.new_state == 100 {
            return Ok(0);
        } else if state.new_state == 120 {
            return Ok(state.reason);
        }
    }
    anyhow::bail!("Unexpected result")
}

fn bssid_to_bytes(bssid: &str) -> Vec<u8> {
    bssid
        .split(':')
        .filter_map(|x| u8::from_str_radix(x, 16).ok())
        .collect()
}

pub async fn forget_config(
    conn: &Connection,
    setting_path: &OwnedObjectPath,
) -> anyhow::Result<()> {
    let setting = zbus::Proxy::new(
        conn,
        "org.freedesktop.NetworkManager",
        setting_path,
        "org.freedesktop.NetworkManager.Settings.Connection",
    )
    .await?;
    setting.call_method("Delete", &()).await?;
    Ok(())
}

pub fn forget_ssid_blocking(con: &blocking::Connection, ssid: &str) -> anyhow::Result<()> {
    let settings_proxy = blocking::Proxy::new(
        &con,
        "org.freedesktop.NetworkManager",
        "/org/freedesktop/NetworkManager/Settings",
        "org.freedesktop.NetworkManager.Settings",
    )?;

    let paths: Vec<OwnedObjectPath> = settings_proxy.call("ListConnections", &())?;

    for setting_path in paths {
        let conn_proxy = blocking::Proxy::new(
            &con,
            "org.freedesktop.NetworkManager",
            &setting_path,
            "org.freedesktop.NetworkManager.Settings.Connection",
        )?;

        let settings: HashMap<String, HashMap<String, OwnedValue>> =
            conn_proxy.call("GetSettings", &())?;

        let connection = settings
            .get("connection")
            .context("Missing 'connection' key")?;

        let conn_ssid: &str = connection
            .get("id")
            .and_then(|v| v.try_into().ok())
            .context("Missing uuid")?;
        let con_type: &str = connection
            .get("type")
            .and_then(|v| v.try_into().ok())
            .context("Missing type")?;

        // Filter uuid.starts_with(utils::UUIDV4_PREFIX) will in future.
        if con_type == "802-11-wireless" && conn_ssid == ssid {
            // forget_config(&con, &path).await?;
            let setting = blocking::Proxy::new(
                con,
                "org.freedesktop.NetworkManager",
                &setting_path,
                "org.freedesktop.NetworkManager.Settings.Connection",
            )?;
            setting.call_method("Delete", &())?;
        }
    }
    anyhow::Ok(())
}

// There is a chance of a use-after-free bug, but it is virtually impossible
// because destroy is currently blocking.
// If destroy were not blocking, this code could receive a network disconnect.
// Then, since remove has already deleted the private data, an undefined behvaiour would occur.
//
// However, this is virtually impossible at the moment.

pub async fn connection_background_task(pd: &'static mut PrivateData) -> anyhow::Result<()> {
    let nm_proxy = Proxy::new(
        &pd.wireless.conn,
        "org.freedesktop.NetworkManager",
        "/org/freedesktop/NetworkManager",
        "org.freedesktop.NetworkManager",
    )
    .await?;

    let mut notifications = nm_proxy.receive_signal("StateChanged").await?;
    while let Some(signal) = notifications.next().await {
        // The signal contains the new state as a u32 (index 0)
        let state: u32 = signal.body().deserialize()?;
        match state {
            70 => (),
            // the disconnection occurred on this interface or another,
            20 => pd.active_connection = None,
            // Will show as connected, even tho, it is connecting, only in external case
            40 if !matches!(pd.state, AppState::Connecting(_))
                || !matches!(pd.state, AppState::PasswordInput { .. }) =>
            {
                pd.active_connection = None
            }
            // A connect event has occured due to external reson.
            _ => {
                continue;
            }
        }

        let Some((bssid, conf)) = get_active_ap(&pd.wireless.conn, &pd.wireless.wifi_proxy).await?
        else {
            continue;
        };

        if let Some(ap) = pd.aps.iter_mut().find(|ap| ap.bssid == bssid) {
            ap.setting_path = Some(conf);
        }

        pd.active_connection = Some(bssid);
        pd.sort_accesspoints();
    }
    Ok(())
}
