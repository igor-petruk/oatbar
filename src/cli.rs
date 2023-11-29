use anyhow::anyhow;
use clap::{Parser, Subcommand};

#[allow(unused)]
mod ipc;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Poke,
    SetVar { name: String, value: String },
    GetVar { name: String },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let response = match cli.command {
        Commands::Poke => ipc::send_request(ipc::Request::Poke),
        Commands::SetVar { name, value } => ipc::send_request(ipc::Request::SetVar { name, value }),
        Commands::GetVar { name } => ipc::send_request(ipc::Request::GetVar { name }),
    }?;
    if let Some(error) = response.error {
        return Err(anyhow!("{}", error));
    }
    if let Some(value) = response.value {
        println!("{}", value);
    }
    Ok(())
}
