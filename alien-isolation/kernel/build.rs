use std::{env, fs, fs::File, io::Write, path::Path};

fn main() {
    println!("cargo:rerun-if-env-changed=PLATFORM");
    println!("cargo:rerun-if-env-changed=KERNEL_BASE");
    println!("cargo:rerun-if-env-changed=VF2_SD");

    let outdir = env::var("OUT_DIR").unwrap();
    let link_script = Path::new(&outdir).join("link.lds");
    let mut script = File::create(&link_script).unwrap();
    let ld_path = Path::new("../tools/link.ld");
    let ld = fs::read_to_string(ld_path).unwrap();
    let platform = env::var("PLATFORM").unwrap_or_else(|_| "qemu_riscv".to_string());

    let base_addr = match env::var("KERNEL_BASE") {
        Ok(value) => Some(parse_address(&value)),
        Err(_) => match platform.as_str() {
            "vf2" => Some(0x4020_0000),
            "k3" => Some(0x1022_0000),
            _ => None,
        },
    };

    if let Some(base_addr) = base_addr {
        let base_addr = format!("BASE_ADDRESS = {:#x};", base_addr);
        let mut new_config = String::new();
        for line in ld.lines() {
            if line.starts_with("BASE_ADDRESS = ") {
                new_config.push_str(base_addr.as_str());
            } else {
                new_config.push_str(line);
            }
            new_config.push('\n');
        }
        script.write_all(new_config.as_bytes()).unwrap();
    } else {
        script.write_all(ld.as_bytes()).unwrap();
    }

    if env::var("VF2_SD").as_deref() == Ok("y") {
        println!("cargo:rustc-cfg=vf2_sd");
    }

    println!("cargo:rustc-link-arg=-T{}", &link_script.display());
    println!("cargo::rustc-cfg={}", platform);
}

fn parse_address(value: &str) -> usize {
    let value = value.trim();
    match value.strip_prefix("0x").or_else(|| value.strip_prefix("0X")) {
        Some(hex) => usize::from_str_radix(hex, 16).expect("KERNEL_BASE is not a valid hex address"),
        None => value.parse().expect("KERNEL_BASE is not a valid address"),
    }
}
