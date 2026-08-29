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
    let _result = match cli.command {
        Command::Send { target } => {
            let _ = (&ssh, target);
            todo!("task 5")
        }
        Command::Setup {
            ssh_alias,
            name,
            force,
        } => {
            let _ = (&ssh, ssh_alias, name, force);
            todo!("task 6")
        }
        Command::Remove { target } => {
            let _ = (&ssh, target);
            todo!("task 6")
        }
    };
}
