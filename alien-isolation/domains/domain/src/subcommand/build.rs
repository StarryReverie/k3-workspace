use std::{collections::HashMap, fs, path::Path};

use crate::subcommand::{Config, DOMAIN_SET};

fn check_output_exist(output: &String) {
    let disk_path = format!("{}/disk", output);
    let init_path = format!("{}/init", output);
    let disk_dir = Path::new(disk_path.as_str());
    let init_dir = Path::new(init_path.as_str());
    if !disk_dir.exists() || !init_dir.exists() {
        println!("Output directory not exist, creating...");
        fs::create_dir_all(&format!("{}/disk", output)).unwrap();
        fs::create_dir_all(&format!("{}/init", output)).unwrap();
    }
}

pub fn build_single(name: &str, log: &str, output: &String, features: &str) {
    check_output_exist(output);
    let domain_list = fs::read_to_string("./domain-list.toml").unwrap();
    let config: Config = toml::from_str(&domain_list).unwrap();
    let all_members = config.domains.get("members").unwrap();
    let r_name = name;
    if !all_members.contains(&r_name.to_string()) {
        println!(
            "Domain [{}] is not in the members list, skip building",
            r_name
        );
        return;
    }
    let extra = if features.is_empty() {
        None
    } else {
        Some(features)
    };
    let init_members = config.domains.get("init_members").unwrap();
    if init_members.contains(&r_name.to_string()) {
        build_domain(r_name, log.to_string(), "init", output, extra);
    } else {
        let disk_members = config.domains.get("disk_members").unwrap();
        if disk_members.contains(&r_name.to_string()) {
            build_domain(r_name, log.to_string(), "disk", output, extra);
        } else {
            println!(
                "Domain [{}] is not in the init or disk members list, skip building",
                r_name
            );
        }
    }
}

pub fn build_domain(name: &str, log: String, dir: &str, output: &String, features: Option<&str>) {
    println!("Building domain [{}] project", name);
    for ty in DOMAIN_SET {
        let path = format!("./{}/{}/g{}/Cargo.toml", ty, name, name);
        let path = Path::new(&path);
        if path.exists() {
            let path = format!("./{}/{}/g{}/Cargo.toml", ty, name, name);
            let path = Path::new(&path);
            println!("Start building domain,path: {:?}", path);
            let mut command = std::process::Command::new("cargo");
            command
                .arg("build")
                .arg("--release")
                .env("LOG", log)
                .arg("--manifest-path")
                .arg(path)
                .arg("--target")
                .arg("./riscv64.json")
                .arg("-Zbuild-std=core,alloc")
                .arg("-Zbuild-std-features=compiler-builtins-mem")
                .arg("--target-dir")
                .arg("./target");
            if let Some(features) = features {
                println!("Extra cargo features for [{}]: {}", name, features);
                command.arg("--features").arg(features);
            }
            let status = command.status().expect("failed to execute cargo build");
            if !status.success() {
                eprintln!("Build domain [{}] failed: {}", name, status);
                std::process::exit(1);
            }
            println!("Build domain [{}] project success", name);
            std::process::Command::new("cp")
                .arg(format!("./target/riscv64/release/g{}", name))
                .arg(format!("{}/{}/g{}", output, dir, name))
                .status()
                .expect("failed to execute cp");
            println!("Copy domain [{}] project success", name);
            return;
        }
    }
}

pub fn build_all(log: String, output: &String, features: &[String]) {
    check_output_exist(output);
    let domain_list = fs::read_to_string("./domain-list.toml").unwrap();
    let config: Config = toml::from_str(&domain_list).unwrap();
    let extra = parse_features(features);
    println!("Start building all domains");
    let all_members = config.domains.get("members").unwrap().clone();
    let init_members = config.domains.get("init_members").unwrap().clone();
    for domain_name in init_members {
        if !all_members.contains(&domain_name) {
            println!(
                "Domain [{}] is not in the members list, skip building",
                domain_name
            );
            continue;
        }
        let value = log.to_string();
        let features = extra.get(domain_name.as_str()).map(|f| f.as_str());
        build_domain(&domain_name, value, "init", output, features)
    }
    let disk_members = config.domains.get("disk_members").unwrap().clone();
    if !disk_members.is_empty() {
        for domain_name in disk_members {
            if !all_members.contains(&domain_name) {
                println!(
                    "Domain [{}] is not in the members list, skip building",
                    domain_name
                );
                continue;
            }
            let value = log.to_string();
            let features = extra.get(domain_name.as_str()).map(|f| f.as_str());
            build_domain(&domain_name, value, "disk", output, features)
        }
    }
}

/// Parse repeatable `NAME=FEATURES` arguments into a per-domain map.
fn parse_features(items: &[String]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for item in items {
        match item.split_once('=') {
            Some((name, features)) if !name.is_empty() && !features.is_empty() => {
                map.insert(name.to_string(), features.to_string());
            }
            _ => {
                eprintln!("--features expects NAME=FEATURES, got {:?}", item);
                std::process::exit(2);
            }
        }
    }
    map
}
