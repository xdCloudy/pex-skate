fn main() {
    if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("wasm32") {
        // The retail physics and animation graph loaders use deeper call chains
        // than a minimal plugin. Keep the guest stack large enough for map/session
        // initialization under PEX's Wasmtime runtime.
        println!("cargo:rustc-link-arg=-zstack-size=8388608");
    }
}
