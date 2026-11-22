use std::{env, fs, path::PathBuf};

fn main() {
    // The kernel and the domains read CPU_NUM at compile time. Track the
    // environment so that switching SMP rebuilds every crate that uses it.
    println!("cargo:rerun-if-env-changed=SMP");
    println!("cargo:rerun-if-env-changed=PLATFORM");

    let cpus = match env::var("SMP") {
        Ok(value) if !value.trim().is_empty() => value
            .trim()
            .parse::<usize>()
            .expect("SMP must be a positive integer"),
        _ => 2,
    };
    assert!(cpus > 0, "SMP must be greater than zero");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is set by Cargo"));
    fs::write(
        out_dir.join("cpu_num.rs"),
        format!("/// 可配置的启动 cpu 数量\npub const CPU_NUM: usize = {cpus};\n"),
    )
    .expect("write cpu_num.rs");

    let platform = env::var("PLATFORM").unwrap_or_else(|_| "qemu_riscv".to_string());
    println!("cargo::rustc-cfg={}", platform);
}
