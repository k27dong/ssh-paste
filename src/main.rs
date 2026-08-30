#![forbid(unsafe_code)]

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(version, about = "Push the local clipboard to remote hosts over ssh")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    #[command(about = "Send the clipboard to a target")]
    Send { target: Option<String> },
    #[command(about = "Install shims on a remote and register it as a target")]
    Setup {
        ssh_alias: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        force: bool,
    },
    #[command(about = "Uninstall shims from a target and forget it")]
    Remove { target: String },
}

fn main() {
    let cli = Cli::parse();
    let ssh = std::env::var_os("SSH_PASTE_SSH").unwrap_or_else(|| "ssh".into());
    let result = match cli.command {
        Command::Send { target } => cmd_send(&ssh, target.as_deref()),
        Command::Setup {
            ssh_alias,
            name,
            force,
        } => cmd_setup(&ssh, &ssh_alias, name.as_deref(), force),
        Command::Remove { target } => cmd_remove(&ssh, &target),
    };
    if let Err(err) = result {
        eprintln!("ssh-paste: {err:#}");
        let code = err
            .downcast_ref::<ssh_paste::ssh::SshFailed>()
            .map(|f| f.0)
            .unwrap_or(1);
        std::process::exit(code);
    }
}

fn cmd_send(ssh: &std::ffi::OsStr, target: Option<&str>) -> anyhow::Result<()> {
    let cfg = ssh_paste::config::load()?;
    let (name, t) = cfg.resolve(target)?;
    let payload = ssh_paste::clipboard::read()?;
    ssh_paste::send::send(ssh, name, t, payload)
}

fn cmd_setup(
    ssh: &std::ffi::OsStr,
    ssh_alias: &str,
    name: Option<&str>,
    force: bool,
) -> anyhow::Result<()> {
    let mut cfg = ssh_paste::config::load()?;
    ssh_paste::setup::setup(ssh, &mut cfg, ssh_alias, name, force)
}

fn cmd_remove(ssh: &std::ffi::OsStr, target: &str) -> anyhow::Result<()> {
    let mut cfg = ssh_paste::config::load()?;
    ssh_paste::setup::remove(ssh, &mut cfg, target)
}
