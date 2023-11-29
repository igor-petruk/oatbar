use anyhow::anyhow;
use clap::{Parser, Subcommand, ValueEnum};

#[allow(unused)]
mod ipc;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Clone, ValueEnum)]
enum Direction {
    Right,
    Left,
}

#[derive(Subcommand)]
enum VarSubcommand {
    Set {
        name: String,
        value: String,
    },
    Get {
        name: String,
    },
    Rotate {
        name: String,
        direction: Direction,
        values: Vec<String>,
    },
}

#[derive(Subcommand)]
enum Commands {
    Poke,
    Var {
        #[clap(subcommand)]
        var: VarSubcommand,
    },
}

fn var_rotate(
    name: String,
    direction: Direction,
    values: Vec<String>,
) -> anyhow::Result<ipc::Response> {
    if values.is_empty() {
        return Err(anyhow::anyhow!("Values list must be not empty"));
    }
    let current_value = ipc::send_request(ipc::Request::GetVar { name: name.clone() })?;
    let position = values
        .iter()
        .enumerate()
        .find(|(_, v)| Some(*v) == current_value.value.as_ref())
        .map(|(idx, _)| idx);
    let last_idx = values.len() - 1;
    use Direction::*;
    let new_idx = match (direction, position) {
        (Left, None) => last_idx,
        (Left, Some(0)) => last_idx,
        (Left, Some(l)) => l - 1,
        (Right, None) => 0,
        (Right, Some(x)) if x == last_idx => 0,
        (Right, Some(l)) => l + 1,
    };
    let new_value = values.get(new_idx).expect("new_idx should be within range");
    ipc::send_request(ipc::Request::SetVar {
        name,
        value: new_value.clone(),
    })
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let response = match cli.command {
        Commands::Poke => ipc::send_request(ipc::Request::Poke),
        Commands::Var { var } => match var {
            VarSubcommand::Set { name, value } => {
                ipc::send_request(ipc::Request::SetVar { name, value })
            }
            VarSubcommand::Get { name } => ipc::send_request(ipc::Request::GetVar { name }),
            VarSubcommand::Rotate {
                name,
                direction,
                values,
            } => var_rotate(name, direction, values),
        },
    }?;
    if let Some(error) = response.error {
        return Err(anyhow!("{}", error));
    }
    if let Some(value) = response.value {
        println!("{}", value);
    }
    Ok(())
}
