use crate::network_manager;

use super::PrivateData;
use super::ffi;
use super::rofi;
use super::structure::*;
use glib::MainContext;
use std::cell::RefCell;
use std::ops::DerefMut;
use std::rc::Rc;
use std::time::Duration;

pub fn handle_state(
    sw: &'static mut ffi::Mode,
    selected_line: usize,
    pd: &'static mut PrivateData,
    input: &std::ffi::CStr,
) -> u32 {
    let rc_sw = Rc::new(RefCell::new(sw));
    match pd.state {
        AppState::Idle => {
            set_wifi_mode_password(rc_sw, pd, selected_line, input, 0);
        }

        AppState::Scanning => {
            set_wifi_mode_password(rc_sw, pd, selected_line, input, 0);
        }

        AppState::PasswordInput { ref bssid, .. } => {
            pd.anim_connecting.index = 0;
            set_mode_connecting_and_handle(rc_sw, pd, bssid.into(), input.into(), 0)
        }
        AppState::Connecting(_) => {
            set_wifi_mode_password(rc_sw, pd, selected_line, input, 0);
        }
    };
    ffi::ModeMode_RESET_DIALOG
}

// seems like incase, if the response wasn't made quick enough or not blocked, then, the
// same events will be fired again by rofi.
pub fn set_wifi_mode_password(
    sw: Rc<RefCell<&'static mut ffi::Mode>>,
    pd: &'static mut PrivateData,
    selected_line: usize,
    input: &std::ffi::CStr,
    reason: u32,
) {
    if pd.state == AppState::Scanning {
        let glib_context = glib::MainContext::default();
        glib_context.block_on(async {
            pd.shut_scan().await;
        });
    } else if pd.state == AppState::Scanning {
        let glib_context = glib::MainContext::default();
        glib_context.block_on(async {
            pd.shut_connect().await;
        });
    }

    if reason == 7 {
        //NM_DEVICE_STATE_REASON_NO_SECRETS
        sw.borrow_mut().display_name = c"bad auth".as_ptr() as *mut i8;
    } else if reason == 0 {
        sw.borrow_mut().display_name = c"password".as_ptr() as *mut i8;
    } else {
        sw.borrow_mut().display_name = c"fail".as_ptr() as *mut i8;
    }

    if selected_line > pd.aps.len() {
        pd.hidden_ssid = Some(input.to_str().unwrap().to_string());
        pd.state = AppState::PasswordInput {
            bssid: input.to_str().unwrap().to_string(),
            reason: reason,
        };
    } else {
        pd.hidden_ssid = None;
        let ap = pd
            .aps
            .get(selected_line)
            .expect("Invalid index in set_wifi_mode_password");

        let sw_rc = Rc::clone(&sw);
        let bssid = ap.bssid.clone();
        if ap.setting_path.is_some() && reason == 0 {
            set_mode_connecting_and_handle(sw_rc, pd, bssid, None, reason);
        } else {
            pd.state = AppState::PasswordInput {
                bssid: bssid,
                reason: reason,
            };
        }
    }
}

pub fn set_wifi_mode_scan(
    sw: Rc<RefCell<&'static mut ffi::Mode>>,
    pd: &mut &'static mut PrivateData,
    // execution_context_signal: Rc<RefCell<u8>>,
) -> glib::SourceId {
    pd.anim_scan.index = 0;
    pd.state = AppState::Scanning;
    let fps = pd.anim_scan.fps;

    // let original_prompt = unsafe { std::ffi::CString::from_raw(sw.display_name) };

    let interval = Duration::from_millis(1000 / fps as u64);
    glib::timeout_add_local(interval, move || {
        // There won't be any race condition between this and .
        let mut state_guard = sw.borrow_mut();
        let state = state_guard.deref_mut();

        let Some(pd) = rofi::get_private_state_mut::<PrivateData>(*state) else {
            return glib::ControlFlow::Break;
        };

        if pd.pool_shut_signal(VFBTask::Scan) {
            return glib::ControlFlow::Break;
        }

        let anim_frame = &pd.anim_scan.frames[pd.anim_scan.index % pd.anim_scan.frames.len()];
        pd.anim_scan.index = pd.anim_scan.index.saturating_add(1);
        rofi::set_prompt(state, anim_frame);
        glib::ControlFlow::Continue
    })
}

// seems like incase, if the response wasn't made quick enough or not blocked, then, the
// same events will be fired again by rofi.

// so, connecting handler has been move to background task
pub fn set_mode_connecting_and_handle(
    sw: Rc<RefCell<&'static mut ffi::Mode>>,
    pd: &'static mut PrivateData,
    bssid: String,
    password: Option<&std::ffi::CStr>,
    reason: u32,
) {
    if pd.state == AppState::Scanning || matches!(pd.state, AppState::Connecting(_)) {
        let glib_context = glib::MainContext::default();
        glib_context.block_on(async {
            pd.shut_scan().await;
        });
    }
    pd.state = AppState::Connecting(bssid.clone());
    pd.active_connection = None;
    let fps = pd.anim_connecting.fps;

    pd.sort_accesspoints();

    sw.borrow_mut().display_name = c"wifi".as_ptr() as *mut i8;

    let interval = Duration::from_millis(1000 / fps as u64);

    let access_point = if pd.hidden_ssid.is_none() {
        // need ownership, but don't wanna rc this rc that
        pd.aps.iter().find(|x| {
            x.bssid == bssid
        }).expect("unexpected error, proabbly perodic scan removed the out of range ap, however, a scan shouldn't have occuered duing this state").clone()
    } else {
        AccessPoint {
            bssid: bssid.clone(),
            frequency: 0,
            is_protected: true,
            signal_strength: 0,
            ssid: bssid.clone(),
            setting_path: None,
        }
    };

    let sw_render = Rc::clone(&sw);
    let glib_context = MainContext::default();

    pd.allow_execute(VFBTask::Connect);
    glib::timeout_add_local(interval, move || {
        let mut state_guard = sw_render.borrow_mut();
        let state = state_guard.deref_mut();

        let Some(pd) = rofi::get_private_state_mut::<PrivateData>(*state) else {
            return glib::ControlFlow::Break;
        };

        if pd.pool_shut_signal(VFBTask::Connect) {
            return glib::ControlFlow::Break;
        }
        pd.anim_connecting.index += 1;
        // rofi::set_prompt(state, anim_frame);
        rofi::reload_view();
        glib::ControlFlow::Continue
    });

    let sw_state = Rc::clone(&sw);
    let own_password = password.map(|p| p.to_string_lossy().to_string());
    glib_context.spawn_local(async move {
        let pd = {
            let mut state_guard = sw_state.borrow_mut();
            let state = state_guard.deref_mut();

            rofi::get_private_state_mut::<PrivateData>(*state)
                .ok_or_else(|| anyhow::anyhow!("Error, failed to get private data"))?
        };

        let wifi_config;
        if access_point.setting_path.is_some() && reason == 0 {
            wifi_config = network_manager::connect_pre_existing_access_point(
                &pd.nm_dbus.con,
                &access_point,
                &pd.nm_dbus.dev_path,
            )
            .await?;
        } else {
            wifi_config = network_manager::create_and_connect_access_point(
                &pd.nm_dbus.con,
                &access_point,
                &pd.nm_dbus.dev_path,
                own_password,
                pd.hidden_ssid.take(),
            )
            .await?;
        }
        let reason = network_manager::network_state(&pd.nm_dbus.con, &pd.nm_dbus.dev_path)
            .await
            .unwrap();
        if reason > 0 {
            network_manager::forget_config(&pd.nm_dbus.con, &wifi_config).await?;
            let index = pd
                .aps
                .iter()
                .position(|a| a.bssid == access_point.bssid)
                .unwrap_or(usize::MAX);

            set_wifi_mode_password(
                Rc::clone(&sw),
                pd,
                index,
                std::ffi::CString::new(bssid.clone()).unwrap().as_c_str(),
                reason,
            );

            // pd.state = AppState::PasswordInput {
            //     bssid: bssid,
            //     reason,
            // };
            // pd.shut_connect().await;
            rofi::view_reset(&mut sw.borrow_mut());
        } else {
            pd.set_connected(Some((bssid, wifi_config)));
            pd.state = AppState::Idle;
            pd.shut_connect().await;

            let mut sw_guard = sw_state.borrow_mut();
            rofi::set_prompt(&mut sw_guard, &std::ffi::CString::new("wifi").unwrap());
            rofi::reload_view();
        }
        anyhow::Ok(())
    });
}

// Experementing this apporach
// glib::MainContext::default().spawn_local(async move {
//     while pd.state == AppState::Scanning {
//         // Wait for next frame
//         glib::timeout_future(Duration::from_millis(100)).await;
//     }
// });
