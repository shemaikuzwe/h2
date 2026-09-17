use crate::apps::Image;
use anyhow::Context;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

pub struct Vm {
    pub name: String,
    pub image: String,
    pub user: String,
    pub cpu: String,
    pub memory: String,
    pub disk: String,
    pub ip: String,
    pub running: bool,
}

pub struct CreateVm {
    pub name: String,
    pub cpu: u32,
    pub memory: u32,
    pub disk: u32,
    pub user: String,
    pub image: Image,
    pub user_data: String,
}

pub struct Snapshot {
    pub name: String,
    pub created_at: String,
}

pub struct Backup {
    pub file: PathBuf,
    pub size_bytes: u64,
    pub created_at: String,
}

pub fn create(spec: &CreateVm) -> anyhow::Result<()> {
    // user/image aren't tracked by incus; keep them as user.* config so list can show them
    incus([
        "launch",
        spec.image.source(),
        &spec.name,
        "--vm",
        "-c",
        &format!("limits.cpu={}", spec.cpu),
        "-c",
        &format!("limits.memory={}MiB", spec.memory),
        "-d",
        &format!("root,size={}GiB", spec.disk),
        "-c",
        &format!("user.image={}", spec.image),
        "-c",
        &format!("user.owner={}", spec.user),
        "-c",
        &format!("cloud-init.user-data={}", spec.user_data),
    ])?;
    Ok(())
}

pub fn list() -> anyhow::Result<Vec<Vm>> {
    let out = incus(["list", "--format", "json"])?;
    let raw: Vec<Instance> = serde_json::from_str(&out)?;
    Ok(raw.into_iter().map(Vm::from).collect())
}

pub fn get(name: &str) -> anyhow::Result<Vm> {
    list()?
        .into_iter()
        .find(|v| v.name == name)
        .with_context(|| format!("no VM found with name '{name}'"))
}

pub fn set(name: &str, key: &str, value: &str) -> anyhow::Result<()> {
    incus(["config", "set", name, &format!("{key}={value}")])?;
    Ok(())
}

/// Grows the root disk; incus refuses to shrink.
pub fn resize(name: &str, disk: u32) -> anyhow::Result<()> {
    incus([
        "config",
        "device",
        "set",
        name,
        "root",
        &format!("size={disk}GiB"),
    ])?;
    Ok(())
}

pub fn start(name: &str) -> anyhow::Result<()> {
    incus(["start", name]).map(drop)
}

pub fn stop(name: &str) -> anyhow::Result<()> {
    incus(["stop", name]).map(drop)
}

pub fn restart(name: &str) -> anyhow::Result<()> {
    incus(["restart", name]).map(drop)
}

pub fn delete(name: &str) -> anyhow::Result<()> {
    incus(["delete", "--force", name])?;
    let d = backup_dir(name);
    if d.exists() {
        fs::remove_dir_all(d)?;
    }
    Ok(())
}

pub fn snapshot_create(vm: &str, name: &str) -> anyhow::Result<()> {
    incus(["snapshot", "create", vm, name]).map(drop)
}

pub fn snapshot_list(vm: &str) -> anyhow::Result<Vec<Snapshot>> {
    let out = incus(["snapshot", "list", vm, "--format", "json"])?;
    let raw: Vec<RawSnapshot> = serde_json::from_str(&out)?;
    Ok(raw
        .into_iter()
        .map(|s| Snapshot {
            name: s.name,
            created_at: s.created_at,
        })
        .collect())
}

pub fn snapshot_restore(vm: &str, name: &str) -> anyhow::Result<()> {
    incus(["snapshot", "restore", vm, name]).map(drop)
}

pub fn snapshot_delete(vm: &str, name: &str) -> anyhow::Result<()> {
    incus(["snapshot", "delete", vm, name]).map(drop)
}

pub fn backup_create(vm: &str) -> anyhow::Result<Backup> {
    let d = backup_dir(vm);
    fs::create_dir_all(&d)?;
    let ts = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
    let file = d.join(format!("{ts}.tar.gz"));
    incus(["export", vm, &file.to_string_lossy(), "--instance-only"])?;
    backup_info(file)
}

pub fn backup_list(vm: &str) -> anyhow::Result<Vec<Backup>> {
    let d = backup_dir(vm);
    if !d.exists() {
        return Ok(vec![]);
    }
    let mut files: Vec<PathBuf> = fs::read_dir(d)?
        .map(|e| Ok(e?.path()))
        .collect::<anyhow::Result<_>>()?;
    files.sort();
    files.into_iter().map(backup_info).collect()
}

/// Replaces the VM with the exported one; VM must be stopped.
pub fn backup_restore(vm: &str, file: &str) -> anyhow::Result<()> {
    let path = backup_path(vm, file)?;
    incus(["delete", vm])?;
    incus(["import", &path.to_string_lossy(), vm])?;
    Ok(())
}

pub fn backup_delete(vm: &str, file: &str) -> anyhow::Result<()> {
    fs::remove_file(backup_path(vm, file)?)?;
    Ok(())
}

fn backup_dir(vm: &str) -> PathBuf {
    let home = std::env::var("HOME").expect("HOME not set");
    PathBuf::from(home).join(".local/share/h2/backups").join(vm)
}

fn backup_path(vm: &str, file: &str) -> anyhow::Result<PathBuf> {
    let p = backup_dir(vm).join(file);
    anyhow::ensure!(p.exists(), "no backup {file} for '{vm}'");
    Ok(p)
}

fn backup_info(file: PathBuf) -> anyhow::Result<Backup> {
    let meta = fs::metadata(&file)?;
    let created: chrono::DateTime<chrono::Local> = meta.modified()?.into();
    Ok(Backup {
        size_bytes: meta.len(),
        created_at: created.format("%Y-%m-%d %H:%M:%S").to_string(),
        file,
    })
}

fn incus<'a>(args: impl IntoIterator<Item = &'a str>) -> anyhow::Result<String> {
    let out = Command::new("incus")
        .args(args)
        .output()
        .context("running incus")?;
    anyhow::ensure!(
        out.status.success(),
        "incus failed: {}",
        String::from_utf8_lossy(&out.stderr).trim()
    );
    Ok(String::from_utf8(out.stdout)?)
}

// only the fields we show from `incus list --format json`
#[derive(Deserialize)]
struct Instance {
    name: String,
    status: String,
    config: HashMap<String, String>,
    expanded_devices: HashMap<String, HashMap<String, String>>,
    state: Option<State>,
}

#[derive(Deserialize)]
struct State {
    network: Option<HashMap<String, Network>>,
}

#[derive(Deserialize)]
struct Network {
    addresses: Vec<Address>,
}

#[derive(Deserialize)]
struct Address {
    family: String,
    address: String,
    scope: String,
}

#[derive(Deserialize)]
struct RawSnapshot {
    name: String,
    created_at: String,
}

impl From<Instance> for Vm {
    fn from(i: Instance) -> Self {
        let cfg = |k: &str| i.config.get(k).cloned().unwrap_or_default();
        let ip = i
            .state
            .and_then(|s| s.network)
            .into_iter()
            .flat_map(|n| n.into_values())
            .flat_map(|n| n.addresses)
            .find(|a| a.family == "inet" && a.scope == "global")
            .map(|a| a.address)
            .unwrap_or_default();
        Vm {
            image: cfg("user.image"),
            user: cfg("user.owner"),
            cpu: cfg("limits.cpu"),
            memory: cfg("limits.memory"),
            disk: i
                .expanded_devices
                .get("root")
                .and_then(|d| d.get("size"))
                .cloned()
                .unwrap_or_default(),
            running: i.status == "Running",
            name: i.name,
            ip,
        }
    }
}
