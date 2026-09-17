mod apps;
mod console;
mod incus;

use anyhow::Context;
use apps::{App, Image};
use clap::{Parser, Subcommand};
use colored::Colorize;

#[derive(Parser)]
#[command(version, about = "Create and manage incus VMs")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[command(about = "Creates and boots a VM (default cpu 1, memory 500MB, disk 10GB)")]
    Create(CreateArgs),
    #[command(about = "Lists all VMs", alias = "ls")]
    List,
    #[command(about = "Updates a VM's cpu/memory/disk (disk can only grow)")]
    Update(UpdateArgs),
    #[command(about = "Deletes a VM and its backups")]
    Delete { name: String },
    #[command(about = "Starts a stopped VM")]
    Run { name: String },
    #[command(about = "Gracefully shuts down a VM")]
    Stop { name: String },
    #[command(about = "Reboots a VM")]
    Reboot { name: String },
    #[command(about = "Opens a web console attached to the VM's serial port")]
    Console {
        name: String,
        #[arg(long, default_value_t = 8080)]
        port: u16,
    },
    #[command(about = "Manages snapshots")]
    Snapshot {
        #[command(subcommand)]
        cmd: SnapshotCmd,
    },
    #[command(about = "Manages backups (restore requires a stopped VM)")]
    Backup {
        #[command(subcommand)]
        cmd: BackupCmd,
    },
}

#[derive(Subcommand)]
enum SnapshotCmd {
    Create {
        vm: String,
        name: String,
    },
    #[command(alias = "ls")]
    List {
        vm: String,
    },
    Restore {
        vm: String,
        name: String,
    },
    Delete {
        vm: String,
        name: String,
    },
}

#[derive(Subcommand)]
enum BackupCmd {
    Create {
        vm: String,
    },
    #[command(alias = "ls")]
    List {
        vm: String,
    },
    Restore {
        vm: String,
        file: String,
    },
    Delete {
        vm: String,
        file: String,
    },
}

#[derive(Parser)]
struct CreateArgs {
    name: String,
    #[arg(long, default_value_t = 1)]
    cpu: u32,
    #[arg(long, default_value_t = 500, help = "Memory in MB")]
    memory: u32,
    #[arg(long, default_value_t = 10, help = "Disk in GB")]
    disk: u32,
    #[arg(long, help = "Default non-root VM user")]
    user: String,
    #[arg(long, value_enum, default_value_t = Image::Ubuntu24)]
    image: Image,
    #[arg(
        long,
        value_enum,
        value_delimiter = ',',
        help = "Apps to install on first boot"
    )]
    apps: Vec<App>,
}

#[derive(Parser)]
struct UpdateArgs {
    name: String,
    #[arg(long)]
    cpu: Option<u32>,
    #[arg(long, help = "Memory in MB")]
    memory: Option<u32>,
    #[arg(long, help = "Disk in GB")]
    disk: Option<u32>,
    #[arg(long, default_value_t = true, help = "Reboot so changes take effect")]
    reboot: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Commands::Create(a) => {
            let home = std::env::var("HOME")?;
            let key = std::fs::read_to_string(format!("{home}/.ssh/id_ed25519.pub"))
                .context("reading ~/.ssh/id_ed25519.pub")?;
            let password = rpassword::prompt_password("User password: ")?;
            let user_data =
                apps::user_data(&a.name, &a.user, &password, key.trim(), a.image, &a.apps)?;
            incus::create(&incus::CreateVm {
                name: a.name.clone(),
                cpu: a.cpu,
                memory: a.memory,
                disk: a.disk,
                user: a.user.clone(),
                image: a.image,
                user_data,
            })?;
            println!("created {}: run `h2 list` for its IP", a.name.green());
        }
        Commands::List => {
            let all = incus::list()?;
            if all.is_empty() {
                println!("no VMs");
                return Ok(());
            }
            println!(
                "{:<12}{:<10}{:<6}{:<10}{:<8}{:<16}{:<10}{}",
                "NAME", "IMAGE", "CPU", "MEMORY", "DISK", "IP", "USER", "STATUS"
            );
            for vm in all {
                let status = if vm.running {
                    "running".green()
                } else {
                    "stopped".red()
                };
                println!(
                    "{:<12}{:<10}{:<6}{:<10}{:<8}{:<16}{:<10}{}",
                    vm.name, vm.image, vm.cpu, vm.memory, vm.disk, vm.ip, vm.user, status
                );
            }
        }
        Commands::Update(a) => {
            if let Some(cpu) = a.cpu {
                incus::set(&a.name, "limits.cpu", &cpu.to_string())?;
            }
            if let Some(mem) = a.memory {
                incus::set(&a.name, "limits.memory", &format!("{mem}MiB"))?;
            }
            if let Some(disk) = a.disk {
                incus::resize(&a.name, disk)?;
            }
            if a.reboot && incus::get(&a.name)?.running {
                incus::restart(&a.name)?;
            }
            println!("updated {}", a.name.green());
        }
        Commands::Delete { name } => {
            incus::delete(&name)?;
            println!("deleted {}", name.red());
        }
        Commands::Run { name } => {
            incus::start(&name)?;
            println!("started {}", name.green());
        }
        Commands::Stop { name } => {
            incus::stop(&name)?;
            println!("stopped {}", name.red());
        }
        Commands::Reboot { name } => {
            incus::restart(&name)?;
            println!("rebooted {}", name.green());
        }
        Commands::Console { name, port } => console::serve(&name, port).await?,
        Commands::Snapshot { cmd } => match cmd {
            SnapshotCmd::Create { vm, name } => {
                incus::snapshot_create(&vm, &name)?;
                println!("snapshot {} created", name.green());
            }
            SnapshotCmd::List { vm } => {
                let all = incus::snapshot_list(&vm)?;
                if all.is_empty() {
                    println!("no snapshots");
                    return Ok(());
                }
                println!("{:<20}{}", "NAME", "CREATED");
                for s in all {
                    println!("{:<20}{}", s.name, s.created_at);
                }
            }
            SnapshotCmd::Restore { vm, name } => {
                incus::snapshot_restore(&vm, &name)?;
                println!("restored {} to {}", vm.green(), name.green());
            }
            SnapshotCmd::Delete { vm, name } => {
                incus::snapshot_delete(&vm, &name)?;
                println!("snapshot {} deleted", name.red());
            }
        },
        Commands::Backup { cmd } => match cmd {
            BackupCmd::Create { vm } => {
                let b = incus::backup_create(&vm)?;
                println!("backup created: {}", b.file.display());
            }
            BackupCmd::List { vm } => {
                let all = incus::backup_list(&vm)?;
                if all.is_empty() {
                    println!("no backups");
                    return Ok(());
                }
                println!("{:<24}{:<10}{}", "FILE", "SIZE", "CREATED");
                for b in all {
                    let file = b.file.file_name().unwrap().to_string_lossy();
                    let size = format!("{} MB", b.size_bytes / 1_000_000);
                    println!("{:<24}{:<10}{}", file, size, b.created_at);
                }
            }
            BackupCmd::Restore { vm, file } => {
                anyhow::ensure!(
                    !incus::get(&vm)?.running,
                    "'{vm}' is running, stop it first"
                );
                incus::backup_restore(&vm, &file)?;
                println!("restored {} from {}", vm.green(), file);
            }
            BackupCmd::Delete { vm, file } => {
                incus::backup_delete(&vm, &file)?;
                println!("backup {} deleted", file.red());
            }
        },
    }
    Ok(())
}
