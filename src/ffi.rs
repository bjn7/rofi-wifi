#![allow(unused)]
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]


use std::{ffi::c_char, os::raw::c_void};
use super::{
    wifi_mode_destory, wifi_mode_get_display_value, wifi_mode_get_num_entries, wifi_mode_init,
    wifi_mode_result, wifi_mode_token_match,
};

include!(concat!(env!("OUT_DIR"), "/binding.rs"));
// include!(concat!(env!("OUT_DIR"), "/helper.rs"));
// include!(concat!(env!("OUT_DIR"), "/mode-private.rs"));

/// Initialize mode
unsafe extern "C" fn wifi_mode_init_callee(sw: *mut Mode) -> i32 {
    // pov: trust me bro, its totally gonna be static
    wifi_mode_init(unsafe { &mut *sw })
}

unsafe extern "C" fn wifi_mode_get_num_entries_callee(sw: *const Mode) -> u32 {
    wifi_mode_get_num_entries(unsafe { &*sw })
}

unsafe extern "C" fn wifi_mode_get_display_value_callee(
    sw: *const Mode,
    selected_line: u32,
    state: *mut i32,
    _attr_list: *mut *mut _GList,
    get_entry: i32,
) -> *mut i8 {
    // let x = unsafe {
    //     glib::translate::from_glib_none::<List::<>>(attr_list);

    // };
    let result = wifi_mode_get_display_value(
        unsafe { &*sw },
        selected_line as usize,
        unsafe { &mut *state },
        get_entry,
    );

    let str_ptr = result.map(|s| {
        let c_str = std::ffi::CString::new(s).expect("string contained internal null byte");
        c_str.into_raw()
    });
    str_ptr.unwrap_or_else(|| std::ptr::null_mut())
}

unsafe extern "C" fn wifi_mode_destory_callee(sw: *mut Mode) {
    wifi_mode_destory(unsafe { &mut *sw });
}

unsafe extern "C" fn wifi_mode_result_callee(
    sw: *mut Mode,
    menu_retv: i32,
    input: *mut *mut i8,
    selected_line: u32,
) -> u32 {
    wifi_mode_result(
        unsafe { &mut *sw },
        menu_retv,
        unsafe { std::ffi::CStr::from_ptr(*input) },
        selected_line as usize,
    )
}

unsafe extern "C" fn wifi_mode_token_match_callee(
    sw: *const Mode,
    tokens: *mut *mut rofi_int_matcher_t,
    index: u32,
) -> i32 {
    wifi_mode_token_match(unsafe { &*sw }, tokens, index as usize)
}

#[unsafe(no_mangle)]
pub static mut mode: Mode = Mode {
    abi_version: ABI_VERSION,
    name: "wifi\0".as_ptr() as *mut c_char,
    cfg_name_key: {
        let mut name = [0i8; 128];
        name[0] = 'w' as c_char;
        name[1] = 'i' as c_char;
        name[2] = 'f' as c_char;
        name[3] = 'i' as c_char;
        name
    },
    type_: ModeType_MODE_TYPE_DMENU,
    _init: Some(wifi_mode_init_callee),
    _get_num_entries: Some(wifi_mode_get_num_entries_callee),
    _destroy: Some(wifi_mode_destory_callee),
    _get_display_value: Some(wifi_mode_get_display_value_callee),
    _token_match: Some(wifi_mode_token_match_callee),
    _result: Some(wifi_mode_result_callee),
    _completer_result: None,
    _preprocess_input: None,
    _get_completion: None,
    _get_icon: None,
    _get_message: None,
    private_data: std::ptr::null_mut() as *mut _,
    free: None,
    _create: None,
    display_name: std::ptr::null_mut() as *mut _,
    ed: std::ptr::null_mut() as *mut c_void,
    fallback_icon_fetch_uid: 0,
    fallback_icon_not_found: 0,
    module: std::ptr::null_mut() as *mut _,
};
