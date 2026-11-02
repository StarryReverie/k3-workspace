fn main() {
    println!("hello from Rust on K3");
    println!("arch = {}", std::env::consts::ARCH);
    println!("os   = {}", std::env::consts::OS);
    println!("RUST_K3_OK");
}
