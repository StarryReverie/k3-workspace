# Domains

Domains are isolated components in AlienOS. Each domain is a separate Rust project that can be loaded/unloaded/update at runtime. 
Domains can be categorized into three types: Common, Fs, and Driver. 

- Common domains are used to provide common functionalities, such as syscall, memory, and process management. 
- Fs domains are used to provide file system functionalities, such as devfs, dynfs, ramfs, fat-vfs, and domainfs. 
- Driver domains are used to provide device driver functionalities, such as uart8250, virtio-net, visionfive2-sd, plic, and rtc.


See [AlienOS](https://github.com/Godones/Alien/tree/isolation) to know how to load/unload/update a domain.

## Introduction


## Create a new Domain

cd to domains directory

1. run cargo command

   ```
   cargo domain new --name {domain_name}
   ```

2. choose the domain type

   ```
   1. Common
   2. Fs
   3. Driver
   ```
3. input the domain interface name

   ```
   {interface_name}
   ```

4. update domain-list.toml


## Build
```
cargo domain --help # Display help
cargo domain build-all -l "" # Build all domains
cargo domain build -n syscall -l "" # Build syscall domain
```
