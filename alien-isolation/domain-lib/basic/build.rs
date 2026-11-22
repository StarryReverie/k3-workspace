fn main() {
    // logging.rs reads LOG with option_env!. Rebuild when the level changes.
    println!("cargo:rerun-if-env-changed=LOG");
}
