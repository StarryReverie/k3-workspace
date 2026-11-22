fn main() {
    // PLATFORM selects the cfg for this crate.
    println!("cargo:rerun-if-env-changed=PLATFORM");
    // LOG is read at compile time by logger.rs.
    println!("cargo:rerun-if-env-changed=LOG");

    let platform = std::env::var("PLATFORM").unwrap_or_else(|_| "qemu_riscv".to_string());
    println!("cargo::rustc-cfg={}", platform);
}
