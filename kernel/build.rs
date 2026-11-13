fn main() {
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("none") {
        println!("cargo:rustc-link-arg=-Tlinker-{arch}.ld");
        println!("cargo:rerun-if-changed=linker-{arch}.ld");
    }
}
