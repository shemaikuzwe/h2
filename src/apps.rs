use clap::ValueEnum;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Image {
    Ubuntu24,
    Ubuntu22,
    Centos10,
}

impl Image {
    /// Cloud variants ship cloud-init, which incus feeds our user-data to.
    pub fn source(self) -> &'static str {
        match self {
            Image::Ubuntu24 => "images:ubuntu/noble/cloud",
            Image::Ubuntu22 => "images:ubuntu/jammy/cloud",
            Image::Centos10 => "images:centos/10-Stream/cloud",
        }
    }
}

impl fmt::Display for Image {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(self.to_possible_value().unwrap().get_name())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum App {
    Docker,
    Nginx,
    Caddy,
}

/// Renders the cloud-config passed to incus as `cloud-init.user-data`.
pub fn user_data(
    name: &str,
    user: &str,
    password: &str,
    key: &str,
    image: Image,
    apps: &[App],
) -> anyhow::Result<String> {
    // ubuntu is apt based, centos is dnf
    let apt = image != Image::Centos10;
    let mut packages = vec![
        "openssh-server",
        "ca-certificates",
        "curl",
        "wget",
        "git",
        "unzip",
        "zip",
        "tar",
        "gzip",
        "bzip2",
        "sudo",
        "tzdata",
    ];
    // names differ between apt and dnf
    packages.extend(if apt {
        ["xz-utils", "gnupg", "locales", "procps", "openssh-client"]
    } else {
        [
            "xz",
            "gnupg2",
            "glibc-langpack-en",
            "procps-ng",
            "openssh-clients",
        ]
    });
    let mut runcmd = Vec::new();
    for app in apps {
        match app {
            App::Nginx => packages.push("nginx"),
            App::Docker => {
                if apt {
                    packages.push("docker.io");
                } else {
                    runcmd.push("curl -fsSL https://get.docker.com | sh".to_string());
                }
                runcmd.push(format!("usermod -aG docker {user}"));
            }
            App::Caddy => {
                anyhow::ensure!(
                    image != Image::Ubuntu22,
                    "caddy is not available for ubuntu22"
                );
                if apt {
                    packages.push("caddy");
                } else {
                    runcmd.push("dnf copr enable -y @caddy/caddy".to_string());
                    runcmd.push("dnf install -y caddy".to_string());
                }
            }
        }
    }
    let mut out = format!(
        "#cloud-config
hostname: {name}
ssh_pwauth: true
users:
  - name: {user}
    sudo: ALL=(ALL) NOPASSWD:ALL
    shell: /bin/bash
    lock_passwd: false
    plain_text_passwd: {password}
    ssh_authorized_keys:
      - {key}
"
    );
    if !packages.is_empty() {
        out.push_str("packages:\n");
        for p in packages {
            out.push_str(&format!("  - {p}\n"));
        }
    }
    if !runcmd.is_empty() {
        out.push_str("runcmd:\n");
        for c in runcmd {
            out.push_str(&format!("  - {c}\n"));
        }
    }
    Ok(out)
}
