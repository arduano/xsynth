use std::env;

fn main() {
    //Rust is fucking stupid... This is the only fucking way to allow this crate to cross-compile for both linux and windows and work on both platforms without issues
    //I blame winmm wrapper for being a piece of shit that forces us to use Ordinals just for it to not crash
    let target_os = env::var("CARGO_CFG_TARGET_OS");
    match target_os.as_ref().map(|x| &**x) {
        Ok("linux") => {}
        Ok("windows") => println!(
            "cargo:rustc-cdylib-link-arg={:}/Ordinals.def",
            env!("CARGO_MANIFEST_DIR")
        ),
        tos => panic!("unknown target os {:?}!", tos),
    }
}
