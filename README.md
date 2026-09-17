# h2

CLI to create and manage VMs on top of [Incus](https://linuxcontainers.org/incus/).
No database: Incus is the state store (`incus list` is the source of truth);
`user`/`image` live as `user.*` config keys on each instance.

```
h2 CLI   ← this repo: thin typed wrapper, shells out to `incus`
   │
 incus    ← VMs, storage pools, snapshots, exports, bridge + DHCP
   │
 QEMU + KVM
```

## Host setup (once)

```bash
# incus + a zfs pool + a managed bridge on the default profile
sudo apt install -y incus incus-agent qemu-system zfsutils-linux
sudo incus admin init --minimal            # creates a `default` dir pool; replaced below
incus storage create h2 zfs size=100GiB    # loop-backed zpool; or source=<existing dataset>
incus profile device set default root pool=h2
incus storage delete default
incus network create incusbr0
incus profile device add default eth0 nic network=incusbr0 name=eth0

# (Optional if docker installed ) docker sets INPUT/FORWARD policy to DROP, which blocks DHCP/NAT for the bridge
sudo iptables -I INPUT -i incusbr0 -j ACCEPT
sudo iptables -I FORWARD -i incusbr0 -j ACCEPT
sudo iptables -I FORWARD -o incusbr0 -j ACCEPT
# persist: sudo apt install iptables-persistent && sudo netfilter-persistent save
```

Images are pulled from the `images:` remote on first use (`ubuntu24`, `ubuntu22`, `centos10`).
Backups are written to `~/.local/share/h2/backups/<vm>/`.

## Usage

```bash
cargo run -- create myvm --cpu 2 --memory 1024 --disk 20 --user bob --apps docker,nginx
cargo run -- list                          # ssh bob@<ip> once cloud-init finishes
cargo run -- update myvm --cpu 4           # reboots by default; --disk can only grow
cargo run -- stop myvm
cargo run -- run myvm
cargo run -- console myvm                  # web serial console at http://127.0.0.1:8080
cargo run -- snapshot create myvm before-upgrade
cargo run -- snapshot restore myvm before-upgrade
cargo run -- backup create myvm            # incus export → tar.gz
cargo run -- backup restore myvm <file>    # VM must be stopped; replaces it
cargo run -- delete myvm
```
