use crate::ffi::{self, Mode, PropertyType_P_INTEGER, PropertyType_P_LIST, PropertyType_P_STRING};
use std::{
    ffi::{CStr, CString},
    ptr::NonNull,
};

unsafe extern "C" {
    unsafe fn rofi_view_reload(); //// https://github.com/davatorium/rofi/discussions/1654
}

pub enum RofiView {}

unsafe extern "C" {
    // https://github.com/davatorium/rofi/blob/next/include/view.h#L209
    fn rofi_view_get_active() -> *mut RofiView;
    //   https://github.com/davatorium/rofi/blob/next/source/view.c#L200
    // fn rofi_view_update_prompt(state: *mut RofiView);

    // Since, rofi_view_update_prompt isn't available, rofi_view_switch_mode internally calls rofi_view_update_prompt, when mode is reload
    fn rofi_view_switch_mode(state: *mut RofiView, mode: *mut Mode);
}

pub fn config_find_widget(name: &str) -> Option<&mut ffi::ConfigEntry> {
    let c_name = CString::new(name).expect("Widget name contained internal null byte");

    let result = unsafe { ffi::rofi_config_find_widget(c_name.as_ptr(), std::ptr::null(), 1) };

    Some(unsafe { NonNull::new(result)?.as_mut() })
}

pub fn theme_find_property<'a>(
    widget: &mut ffi::ConfigEntry,
    ptype: ffi::PropertyType,
    property: &str,
) -> Option<&'a mut ffi::Property> {
    let c_property = CString::new(property).expect("Proeprty name contained internal null byte");
    let result =
        unsafe { ffi::rofi_theme_find_property(widget as *mut _, ptype, c_property.as_ptr(), 1) };
    Some(unsafe { NonNull::new(result)?.as_mut() })
}

pub fn theme_find_property_int<'a>(widget: &mut ffi::ConfigEntry, property: &str) -> Option<i32> {
    let property = theme_find_property(widget, PropertyType_P_INTEGER, property);
    Some(unsafe { property?.value.i })
}

pub fn theme_find_property_array(
    widget: &mut ffi::ConfigEntry,
    property: &str,
) -> Option<Vec<&'static str>> {
    let property = theme_find_property(widget, PropertyType_P_LIST, property);
    let mut current = unsafe { property?.value.list as *mut glib::ffi::GList };
    let mut result: Vec<&str> = Vec::new();

    while let Some(node) = unsafe { current.as_ref() } {
        let elem_ptr = node.data as *const ffi::Property;
        // skip non string node
        if unsafe { (*elem_ptr).type_ } != PropertyType_P_STRING {
            current = node.next;
            continue;
        }

        if let Some(cstr) = unsafe { CStr::from_ptr((*elem_ptr).value.s).to_str().ok() } {
            result.push(cstr);
        }

        current = node.next;
    }
    Some(result)
}

pub fn find_arg_str(arg: &str) -> Option<String> {
    let c_arg = CString::new(arg).expect("Proeprty name contained internal null byte");

    let mut value_ptr: *mut i8 = std::ptr::null_mut();

    let result = unsafe { ffi::find_arg_str(c_arg.as_ptr(), &mut value_ptr) };
    if result != 0 {
        unsafe {
            let c_str = CStr::from_ptr(value_ptr);
            Some(c_str.to_string_lossy().into())
        }
    } else {
        None
    }
}

pub fn get_private_state<T>(sw: &ffi::Mode) -> Option<&'static T> {
    let result: *mut std::ffi::c_void = unsafe { ffi::mode_get_private_data(sw as *const _) };
    Some(unsafe { NonNull::new(result as *mut T)?.as_ref() })
}

pub fn get_private_state_mut<T>(sw: &mut ffi::Mode) -> Option<&'static mut T> {
    let result: *mut std::ffi::c_void = unsafe { ffi::mode_get_private_data(sw as *const _) };
    Some(unsafe { NonNull::new(result as *mut T)?.as_mut() })
}

pub fn set_private_state<T>(sw: &&mut Mode, pd: &&'static mut T) {
    unsafe {
        ffi::mode_set_private_data(
            *sw as *const _ as *mut _,
            *pd as *const _ as *mut std::ffi::c_void,
        )
    };
}

/// Takes the private state out of the mode, leaving private state with null ptr.
pub fn take_private_state<T>(sw: &mut ffi::Mode) -> Option<Box<T>> {
    let result = unsafe { ffi::mode_get_private_data(sw as *const _) };
    unsafe {
        ffi::mode_set_private_data(sw as *mut _, std::ptr::null_mut());
    }
    Some(unsafe { Box::from_raw(NonNull::new(result as *mut T)?.as_ptr()) })
}

pub fn helper_token_match(tokens: *mut *mut ffi::rofi_int_matcher_t, c_ssid: CString) -> i32 {
    unsafe { ffi::helper_token_match(tokens, c_ssid.as_ptr()) }
}

pub fn reload_view() {
    unsafe { rofi_view_reload() }
}

pub fn set_prompt(sw: &mut ffi::Mode, text: &CString) {
    sw.display_name = text.as_ptr() as *mut i8;
    unsafe { rofi_view_switch_mode(rofi_view_get_active(), sw as *mut _) };
}

pub fn view_reset(sw: &mut ffi::Mode) {
    unsafe { rofi_view_switch_mode(rofi_view_get_active(), sw as *mut _) }
}

#[allow(unused)]
pub fn get_display_name(sw: &mut ffi::Mode) -> CString {
    unsafe { CString::from_raw(sw.display_name) }
}
