mod ffi;
mod rofi;
mod structure;
mod utils;
use std::{cell::RefCell, rc::Rc, time::Duration};
use structure::*;

// mod utils;
use ffi::Mode;
use glib::MainContext;
mod network_manager;
mod state;

use crate::{
    ffi::{MenuReturn_MENU_CUSTOM_INPUT, ModeMode_RELOAD_DIALOG},
    state::handle_state,
};

//  I was just creating a simple prototype and playing around with Rofi without involving much async,
// but somehow it turned into an actual useable plugin with all these background tasks and async.

// I initially planned to just use nmcli to connect and disconnect, but ended up using dbus to directly communicate with NetworkManager.

// Todo!(): Add custom prompt.
// Todo!(): Modiy the wifi-icon icon color, including states color.

fn wifi_mode_init(sw: &'static mut Mode) -> i32 {
    if rofi::get_private_state::<PrivateData>(&sw).is_some() {
        return 1;
    }

    let Some(interface) = rofi::find_arg_str("-iface") else {
        eprintln!("Interface is required");
        return 0;
    };

    let glib_context = MainContext::default();
    let async_block_result = glib_context.block_on(async {
        let network_manager_proxy = network_manager::setup_dbus(&interface).await?;

        let cached_aps = network_manager::fetch_aps(
            &network_manager_proxy.con,
            &network_manager_proxy.wifi_proxy,
        )
        .await?;

        let active_ap_bssid_opt = network_manager::get_active_ap(
            &network_manager_proxy.con,
            &network_manager_proxy.wifi_proxy,
        )
        .await?;

        anyhow::Ok((network_manager_proxy, cached_aps, active_ap_bssid_opt))
    });

    let Ok((network_manager_proxy, cached_aps, active_ap_bssid_opt)) = async_block_result else {
        eprintln!("Failed to create a dbus proxy");
        return 0;
    };

    let mut pd = PrivateData::new(network_manager_proxy, cached_aps);
    pd.set_connected(active_ap_bssid_opt);
    pd.sort_accesspoints();

    // If the 'wifi' widget is found, load the theme properties
    if let Some(theme_widget) = rofi::config_find_widget("wifi") {
        // Load scan's config

        // Load scan's configuration properties
        if let Some(fps) = rofi::theme_find_property_int(theme_widget, "state-scan-fps")
            .filter(|&fps| fps > 0 && fps <= 60)
        {
            pd.anim_scan.fps = fps as u8;
        }

        if let Some(state_scan_indicator) =
            rofi::theme_find_property_array(theme_widget, "state-scan-indicator")
        {
            pd.anim_scan.frames = state_scan_indicator
                .iter()
                .map(|x| IndicatorAnim::build_scan("wifi", x))
                .collect();
        }

        // Load connecting's configuration properties
        if let Some(fps) = rofi::theme_find_property_int(theme_widget, "sate-connecting-fps")
            .filter(|&fps| fps > 0 && fps <= 60)
        {
            pd.anim_scan.fps = fps as u8;
        }

        if let Some(state_scan_indicator) =
            rofi::theme_find_property_array(theme_widget, "sate-connecting-indicator")
        {
            pd.icons.open = state_scan_indicator
                .iter()
                .map(|x| x.chars().next().unwrap_or('￼'))
                .collect();
        }

        // Load icon settings for open and PSK states
        if let Some(wifi_icon_open) =
            rofi::theme_find_property_array(theme_widget, "icon-open").filter(|arr| arr.len() == 5)
        {
            pd.icons.open = wifi_icon_open
                .iter()
                .map(|x| x.chars().next().unwrap_or('￼'))
                .collect();
        }

        if let Some(wifi_icon_close) =
            rofi::theme_find_property_array(theme_widget, "icon-psk").filter(|arr| arr.len() == 5)
        {
            pd.icons.psk = wifi_icon_close
                .iter()
                .map(|x| x.chars().next().unwrap_or('￼'))
                .collect();
        }
    };

    let boxed_pd = Box::new(pd);

    // Avoding smart pointer for this portotype
    // Leaking the memory so it can be used across multiple functions without rust freeing it.
    // This is because `pd` requires manual memory management.
    // let raw_ptr = Box::into_raw(boxed_pd);
    let leaked_pd: &'static mut PrivateData = Box::leak(boxed_pd);

    // For now, let's avoid smart pointer for private data, and store it directly.
    // From future, that was bad idea, todo!():switch to rc.
    rofi::set_private_state::<PrivateData>(&sw, &leaked_pd);

    let sw_rc = Rc::new(RefCell::new(sw));

    let sw_connect_detection_task = Rc::clone(&sw_rc);
    MainContext::default().spawn_local(async move {
        // let mut pd: &'static mut PrivateData = leaked_pd;
        let pd: &'static mut PrivateData = {
            let mut sw = sw_connect_detection_task.borrow_mut();
            rofi::get_private_state_mut(&mut sw).expect("Failed to get private data.")
        };
        let _ = network_manager::connection_background_task(pd).await;
        ()
    });

    let scan = move || {
        let sw = Rc::clone(&sw_rc);
        // let mut pd: &'static mut PrivateData = leaked_pd;
        glib::MainContext::default().spawn_local(async move {
            // let sw: &'staticmut ffi::rofi_mode = sw
            let mut pd: &'static mut PrivateData = {
                let mut sw_guard = sw.borrow_mut();
                rofi::get_private_state_mut(&mut sw_guard).unwrap()
            };

            if pd.state != AppState::Idle {
                println!("Not idle");
                return;
            }

            // Latest: moved to private data.

            // Who needs a channel when you can just talk with bools, lol.
            // When flipped to 1, the other function will shut down gracefully.
            // Why gracefully? Because it *might* override the prompt set by this function.
            // and, after fliping to 1, it must be fliped to 2 to mark a gracefull shutdown.

            // meh, should’ve probably used a channel instead.

            // SWITCH TO: APPLICATION::SCANNING(SCANSIG)
            // NAHH, switch to: private data -> scan sig.
            // let execution_context_signal = Rc::new(RefCell::new(0u8));

            // render_source.remove(), now signal is used to stop the function

            // let original_display_name = rofi::get_display_name(Rc::clone(&sw_rc))

            pd.allow_execute(VFBTask::Scan);
            state::set_wifi_mode_scan(Rc::clone(&sw), &mut pd);

            if let Err(e) =
                network_manager::trigger_rescan(&pd.nm_dbus.property_proxy, &pd.nm_dbus.wifi_proxy)
                    .await
            {
                eprintln!("Failed to scan ap: {}", e);
            }

            if pd.state != AppState::Scanning {
                // There could be a mode switch here while resolving the new scan, which could introduce a bug.
                // If a Wi-Fi network were nearly in range, but the range changed in the middle of the password state,
                // and since that Wi-Fi is no longer being captured, it won't be in the APs list,
                // which may result in an out-of-bounds error, as many access points aren't being checked.

                // therefore, simply don't add the updated list
                return;
            }
            pd.shut_scan().await; //wait for gracefull shutdown of the function

            if let Ok(scanned_aps) =
                network_manager::fetch_aps(&pd.nm_dbus.con, &pd.nm_dbus.wifi_proxy).await
            {
                // pd.aps.append(&mut scanned_aps);
                pd.aps = scanned_aps;
                pd.sort_accesspoints();
            }
            pd.state = AppState::Idle;
            let mut sw_guard = sw.borrow_mut();
            rofi::set_prompt(&mut sw_guard, &pd.display_name);
        })
    };

    scan(); // initiall run

    glib::timeout_add_local(Duration::from_secs(10), move || {
        scan();
        glib::ControlFlow::Continue
    });

    1
}

fn wifi_mode_get_num_entries(sw: &Mode) -> u32 {
    rofi::get_private_state::<PrivateData>(sw).map_or(0, |pd| {
        if matches!(pd.state, AppState::PasswordInput { .. }) {
            0
        } else {
            pd.aps.len() as u32
        }
    })
}

fn wifi_mode_destory(sw: &mut Mode) {
    if let Some(_) = rofi::get_private_state::<PrivateData>(sw) {
        let _ = rofi::take_private_state::<PrivateData>(sw); // leaked mem is back to rust's gc and pd will be droped after this scope
    };
}

fn wifi_mode_get_display_value(
    sw: &Mode,
    selected_line: usize,
    state: &mut i32,
    get_entry: i32,
) -> Option<String> {
    if get_entry == 0 {
        return None;
    }

    let pd = rofi::get_private_state::<PrivateData>(sw)?;
    let ap = pd.aps.get(selected_line)?;
    let icons = if ap.is_protected {
        &pd.icons.psk
    } else {
        &pd.icons.open
    };

    let icon = match ap.signal_strength {
        70..=100 => icons[0], //exec
        50..=69 => icons[1],  //good
        30..=49 => icons[2],  // fair
        10..=29 => icons[3],  // weak
        _ => icons[4],        // very weak
    };

    let sub_label = if let AppState::Connecting(ref b) = pd.state
        && *b == ap.bssid
    {
        *state |= 4 | 8; // Active | Markup
        let anim_frame =
            &pd.anim_connecting.frames[pd.anim_connecting.index % pd.anim_connecting.frames.len()]; // index will be updated in async task froms handle_state       
        Some(anim_frame.to_string_lossy().to_string())
    } else if let Some(ref b) = pd.active_connection
        && *b == ap.bssid
    {
        *state |= 4 | 8; // Active | Markup
        Some("(contected)".into())
    } else {
        None
    };

    match sub_label {
        Some(text) => Some(format!(
            // TODO!: add customization
            "{icon}  {ssid} <span size='small' foreground='#639ec5ff' alpha='80%'>{text}</span>",
            // "{icon}  {ssid} {text}",
            ssid = ap.ssid
        )),
        None => Some(format!("{icon}  {ssid}", ssid = ap.ssid)),
    }
}

fn wifi_mode_token_match(
    sw: &Mode,
    tokens: *mut *mut ffi::rofi_int_matcher_t,
    index: usize,
) -> i32 {
    let match_result = rofi::get_private_state::<PrivateData>(sw)
        .and_then(|pd: &'static PrivateData| {
            if matches!(pd.state, AppState::PasswordInput { .. }) {
                None
            } else {
                pd.aps.get(index)
            }
        })
        .map(|entry| {
            let c_ssid = std::ffi::CString::new(entry.ssid.as_str())
                .expect("SSID contained internal null byte");
            rofi::helper_token_match(tokens, c_ssid)
        });
    match_result.unwrap_or(0)
}

fn wifi_mode_result(
    sw: &'static mut Mode,
    menu_retv: i32,
    input: &std::ffi::CStr,
    selected_line: usize,
) -> u32 {
    let menu_retv = menu_retv as u32;
    let Some(pd) = rofi::get_private_state_mut::<PrivateData>(sw) else {
        return ffi::ModeMode_MODE_EXIT;
    };

    match menu_retv {
        retv if retv & ffi::MenuReturn_MENU_NEXT != 0 => ffi::ModeMode_NEXT_DIALOG,
        retv if retv & ffi::MenuReturn_MENU_PREVIOUS != 0 => ffi::ModeMode_PREVIOUS_DIALOG,
        retv if retv & ffi::MenuReturn_MENU_QUICK_SWITCH != 0 => {
            retv & ffi::MenuReturn_MENU_LOWER_MASK
        }
        retv if retv & ffi::MenuReturn_MENU_OK != 0 => handle_state(sw, selected_line, pd, input),
        retv if retv & ffi::MenuReturn_MENU_ENTRY_DELETE != 0 => {
            if let Some(ap) = pd.aps.get(selected_line)
                && ap.setting_path.is_some()
            {
                // cloning of connection is cheap
                let _ =
                    network_manager::forget_ssid_blocking(&pd.nm_dbus.con.clone().into(), &ap.ssid);
                pd.aps[selected_line].setting_path = None;
            }
            ModeMode_RELOAD_DIALOG
        }
        retv if retv & MenuReturn_MENU_CUSTOM_INPUT != 0 => {
            handle_state(sw, selected_line, pd, input)
        }
        _ => {
            if matches!(pd.state, AppState::PasswordInput { .. }) {
                pd.state = AppState::Idle;
                sw.display_name = c"wifi".as_ptr() as *mut i8;
                return ffi::ModeMode_RELOAD_DIALOG;
            }
            ffi::ModeMode_MODE_EXIT
        }
    }
}
