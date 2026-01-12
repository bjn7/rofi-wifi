use std::{env};
use std::path::PathBuf;

// tried doing it individually, but it messed up the Mode structure, so switched to wrapper.h.
// Commented in case it's needed in the future

// const HEADERS: [&str; 3] = [
//     "/usr/include/rofi/mode.h",
//     "/usr/include/rofi/mode-private.h",
//     "/usr/include/rofi/helper.h",
//     ];

fn main() {
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());

    // for header_path in HEADERS {
        // let header_name = header_path
        //     .rsplit('/')
        //     .next()
        //     .and_then(|s| s.strip_suffix(".h"))
        //     .map(|name| format!("{}.rs", name))
        //     .expect("Invalid header");

        bindgen::Builder::default()
            .header("wrapper.h")
            .clang_arg("-I/usr/include/glib-2.0")
            .clang_arg("-I/usr/lib/glib-2.0/include")
            .clang_arg("-I/usr/include/cairo")
            .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
            .generate()
            .expect("Unable to generate bindings")
            .write_to_file(out_path.join("binding.rs"))
            .expect("Couldn't write bindings!");
}
