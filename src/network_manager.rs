use crate::{
    structure::{AccessPoint, AppState, BSSID, NetworkManagerDbusProxy, PrivateData},
    utils,
};
use anyhow::{self, Context};
use futures_util::{StreamExt, try_join};
use std::collections::HashMap;
use zbus::{
    Connection, Proxy, blocking,
    fdo::PropertiesProxy,
    zvariant::{Array, ObjectPath, OwnedObjectPath, OwnedValue, Value},
};

// well, maybe should have just used zbus-xmlgen...

// Using async so, the scanning animation can be shown while, zbus requests to networkmanager for rescan.
pub async fn setup_dbus(iface: &str) -> anyhow::Result<NetworkManagerDbusProxy> {
    let con = Connection::system().await?;
    let nm_proxy = Proxy::new(
        &con,
        "org.freedesktop.NetworkManager",
        "/org/freedesktop/NetworkManager",
        "org.freedesktop.NetworkManager",
    )
    .await?;

    // get all network devices
    // Using Deperacted "GetDevices"
    let devices: Vec<zbus::zvariant::OwnedObjectPath> = nm_proxy.call("GetDevices", &()).await?;

    for dev_path in devices.into_iter() {
        let dev_proxy = Proxy::new(
            &con,
            "org.freedesktop.NetworkManager",
            &dev_path,
            "org.freedesktop.NetworkManager.Device",
        )
        .await?;

        let iface_name: String = dev_proxy.get_property("Interface").await?;
        let dev_type: u32 = dev_proxy.get_property("DeviceType").await?;

        if dev_type == 2 && iface_name == iface {
            let wifi_proxy = Proxy::new(
                &con,
                "org.freedesktop.NetworkManager",
                &*dev_path,
                "org.freedesktop.NetworkManager.Device.Wireless",
            )
            .await?;

            let property_proxy =
                PropertiesProxy::new(&con, "org.freedesktop.NetworkManager", &*dev_path).await?;

            return Ok(NetworkManagerDbusProxy {
                con,
                dev_path,
                property_proxy,
                wifi_proxy,
            });
        }
    }
    anyhow::bail!("Failed to create dbus proxies")
}

pub async fn trigger_rescan(
    property_proxy: &PropertiesProxy<'static>,
    wifi_proxy: &Proxy<'static>,
) -> anyhow::Result<()> {
    let mut stream = property_proxy.receive_properties_changed().await?;
    let _: () = wifi_proxy
        .call(
            "RequestScan",
            &HashMap::<String, zbus::zvariant::Value>::new(),
        )
        .await?;

    while let Some(s) = stream.next().await {
        if s.args()?.changed_properties.contains_key("LastScan") {
            return Ok(());
        }
    }
    anyhow::bail!("Expected Some, got None")
}

pub async fn fetch_aps(
    conn: &Connection,
    wifi_proxy: &Proxy<'_>,
) -> anyhow::Result<Vec<AccessPoint>> {
    let raw_aps: Vec<zbus::zvariant::OwnedObjectPath> =
        wifi_proxy.call("GetAllAccessPoints", &()).await?;

    let mut aps = Vec::with_capacity(raw_aps.len());
    for ap_path in raw_aps {
        let ap_proxy = Proxy::new(
            conn,
            "org.freedesktop.NetworkManager",
            &ap_path,
            "org.freedesktop.NetworkManager.AccessPoint",
        )
        .await?;

        let (wpa_flags, rsn_flags, ssid, signal_strength, bssid, frequency, auth_flag): (
            u32,
            u32,
            Vec<u8>,
            u8,
            String,
            u32,
            u32,
        ) = try_join!(
            ap_proxy.get_property::<u32>("WpaFlags"),
            ap_proxy.get_property::<u32>("RsnFlags"),
            ap_proxy.get_property::<Vec<u8>>("Ssid"),
            ap_proxy.get_property::<u8>("Strength"),
            ap_proxy.get_property::<String>("HwAddress"),
            ap_proxy.get_property::<u32>("Frequency"),
            ap_proxy.get_property::<u32>("Flags"),
        )?;

        // https://people.freedesktop.org/~lkundrak/nm-dbus-api/nm-dbus-types.html#NM80211ApSecurityFlags

        if (wpa_flags & 512 == 512) || (rsn_flags & 512 == 512) {
            //flag 256 is NM_802_11_AP_SEC_KEY_MGMT_802_1X
            continue;
        }
        //
        aps.push(AccessPoint {
            bssid,
            frequency,
            is_protected: auth_flag & 1 == 1, //https://people.freedesktop.org/~lkundrak/nm-dbus-api/nm-dbus-types.html#NM80211ApFlags
            signal_strength,
            ssid: String::from_utf8_lossy(&ssid).to_string(),
            setting_path: None,
        });
    }

    // Looking into all the saved connections.
    let settings_proxy = Proxy::new(
        &conn,
        "org.freedesktop.NetworkManager",
        "/org/freedesktop/NetworkManager/Settings",
        "org.freedesktop.NetworkManager.Settings",
    )
    .await?;

    let paths: Vec<OwnedObjectPath> = settings_proxy.call("ListConnections", &()).await?;

    for path in paths {
        let conn_proxy = Proxy::new(
            &conn,
            "org.freedesktop.NetworkManager",
            &path,
            "org.freedesktop.NetworkManager.Settings.Connection",
        )
        .await?;

        let settings: HashMap<String, HashMap<String, OwnedValue>> =
            conn_proxy.call("GetSettings", &()).await?;

        let connection = settings
            .get("connection")
            .context("Missing 'connection' key")?;

        let uuid: &str = connection
            .get("uuid")
            .and_then(|v| v.try_into().ok())
            .context("Missing uuid")?;
        let con_type: &str = connection
            .get("type")
            .and_then(|v| v.try_into().ok())
            .context("Missing type")?;

        if uuid.starts_with(utils::UUIDV4_PREFIX) && con_type == "802-11-wireless" {
            let Some(raw_bssid) = settings
                .get("802-11-wireless")
                .and_then(|x| x.get("bssid"))
                .and_then(|v| v.downcast_ref::<Array>().ok())
                .map(|v| v.to_vec())
            else {
                continue;
            };

            let bssid_bytes = raw_bssid
                .iter()
                .filter_map(|x| x.downcast_ref::<u8>().ok())
                .collect::<Vec<u8>>();

            let bssid = bssid_bytes
                .iter()
                .map(|v| format!("{:02X}", v))
                .collect::<Vec<String>>()
                .join(":");

            for ap in &mut aps {
                if ap.bssid == bssid {
                    ap.setting_path = Some(path.to_owned());
                }
            }
        }
    }

    Ok(aps)
}

pub async fn get_active_ap(
    conn: &zbus::Connection,
    wifi_dev_proxy: &Proxy<'static>,
) -> anyhow::Result<Option<(BSSID, OwnedObjectPath)>> {
    // let active_ap_path: OwnedObjectPath = wifi_dev_proxy.get_property("ActiveAccessPoint").await?;

    let device_proxy = Proxy::new(
        &conn,
        "org.freedesktop.NetworkManager",
        wifi_dev_proxy.path(),
        "org.freedesktop.NetworkManager.Device",
    )
    .await?;

    let active_ap_path: OwnedObjectPath = device_proxy.get_property("ActiveConnection").await?;

    if active_ap_path.as_str() == "/" {
        Ok(None)
    } else {
        let active_conn_proxy = Proxy::new(
            conn,
            "org.freedesktop.NetworkManager",
            &active_ap_path,
            "org.freedesktop.NetworkManager.Connection.Active",
        )
        .await?;

        let settings_path: OwnedObjectPath = active_conn_proxy.get_property("Connection").await?;
        let specific_obj: OwnedObjectPath =
            active_conn_proxy.get_property("SpecificObject").await?;

        let ap_proxy = Proxy::new(
            &conn,
            "org.freedesktop.NetworkManager",
            &specific_obj,
            "org.freedesktop.NetworkManager.AccessPoint",
        )
        .await?;
        // .await?;
        let bssid: String = ap_proxy.get_property("HwAddress").await?;
        Ok(Some((bssid, settings_path)))
    }
}

pub async fn connect_pre_existing_access_point(
    conn: &Connection,
    access_point: &AccessPoint,
    dev_path: &OwnedObjectPath,
) -> anyhow::Result<OwnedObjectPath> {
    let nm_proxy = Proxy::new(
        conn,
        "org.freedesktop.NetworkManager",
        "/org/freedesktop/NetworkManager",
        "org.freedesktop.NetworkManager",
    )
    .await?;

    let active_ap_path: OwnedObjectPath = nm_proxy
        .call(
            "ActivateConnection",
            &(
                &access_point
                    .setting_path
                    .as_ref()
                    .expect("setting path is required"),
                dev_path,
                ObjectPath::try_from("/")?,
            ),
        )
        .await?;

    let active_conn_proxy = Proxy::new(
        conn,
        "org.freedesktop.NetworkManager",
        &active_ap_path,
        "org.freedesktop.NetworkManager.Connection.Active",
    )
    .await?;

    let settings: OwnedObjectPath = active_conn_proxy.get_property("Connection").await?;
    Ok(settings)
}


// todo!(): Remove the duct tape and handle hidden Wi-Fi properly.
pub async fn create_and_connect_access_point(
    conn: &Connection,
    access_point: &AccessPoint,
    dev_path: &OwnedObjectPath,
    password: Option<String>,
    hidden: Option<String>,
) -> anyhow::Result<OwnedObjectPath> {
    let nm_proxy = Proxy::new(
        &conn,
        "org.freedesktop.NetworkManager",
        "/org/freedesktop/NetworkManager",
        "org.freedesktop.NetworkManager",
    )
    .await?;
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

    let path = ObjectPath::try_from("/")?;
    let body = (connection_settings, dev_path, &path);

    // maybe should switch to AddAndActivateConnection2
    let message = nm_proxy
        .call_method("AddAndActivateConnection", &body)
        .await?
        .body();
    let (sys, _): (OwnedObjectPath, OwnedObjectPath) = message.deserialize()?;

    Ok(sys.to_owned())
}

pub async fn network_state<'a>(
    conn: &Connection,
    // active_conn_path: ObjectPath<'a>,
    device_path: &OwnedObjectPath,
) -> anyhow::Result<u32> {
    // let active_proxy = Proxy::new(
    //     &conn,
    //     "org.freedesktop.NetworkManager",
    //     active_conn_path,
    //     "org.freedesktop.NetworkManager.Connection.Active",
    // )
    // .await?;

    let device_proxy = Proxy::new(
        conn,
        "org.freedesktop.NetworkManager",
        device_path,
        "org.freedesktop.NetworkManager.Device",
    )
    .await?;
    let mut device_signals = device_proxy.receive_signal("StateChanged").await?;
    while let Some(mg) = device_signals.next().await {
        let (new_state, _old_state, reason): (u32, u32, u32) = mg.body().deserialize()?;
        if new_state == 100 {
            return Ok(0);
        } else if new_state == 120 {
            return Ok(reason);
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
        &pd.nm_dbus.con,
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

        let Some((bssid, conf)) = get_active_ap(&pd.nm_dbus.con, &pd.nm_dbus.wifi_proxy).await?
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
