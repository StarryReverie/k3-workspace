use std::{fs, path::Path};

fn main() {
    let cpus = option_env!("SMP");

    if let Some(cpus) = cpus {
        let cpus = cpus.parse::<usize>().unwrap();
        let config_file = Path::new("src/lib.rs");
        let config = fs::read_to_string(config_file).unwrap();
        let cpus = format!("pub const CPU_NUM: usize = {};\n", cpus);
        let mut new_config = String::new();
        for line in config.lines() {
            if line.starts_with("pub const CPU_NUM: usize = ") {
                new_config.push_str(cpus.as_str());
            } else {
                new_config.push_str(line);
                new_config.push('\n');
            }
        }
        fs::write(config_file, new_config).unwrap();
    }

    println!("cargo:rerun-if-changed=src/lib.rs");
    let platform = option_env!("PLATFORM").unwrap_or("qemu_riscv");
    println!("cargo::rustc-cfg={}", platform);
}
