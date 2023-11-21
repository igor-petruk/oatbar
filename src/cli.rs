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
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let response = match cli.command {
        Commands::Poke => ipc::send_request(ipc::Request::Poke),
        Commands::SetVar { name, value } => match name.split_once(':') {
            Some((command_name, name)) => ipc::send_request(ipc::Request::SetVar {
                command_name: command_name.into(),
                name: name.into(),
                value,
            }),
            None => return Err(anyhow!("--name must be in the 'command:name' format")),
        },
    }?;
    if let Some(error) = response.error {
        return Err(anyhow!("{}", error));
    }
    Ok(())
}
