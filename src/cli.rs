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
    #[command(name = "ls")]
    List {},
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
    let response = ipc::send_command(ipc::Command::GetVar { name: name.clone() })?;
    if let Some(error) = response.error {
        return Err(anyhow!("{}", error));
    }
    let value = match response.data {
        Some(ipc::ResponseData::Value(value)) => value,
        x => return Err(anyhow!("Unexpected response: {:?}", x)),
    };
    let position = values
        .iter()
        .enumerate()
        .find(|(_, v)| *v == &value)
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
    ipc::send_command(ipc::Command::SetVar {
        name,
        value: new_value.clone(),
    })
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let response = match cli.command {
        Commands::Poke => ipc::send_command(ipc::Command::Poke),
        Commands::Var { var } => match var {
            VarSubcommand::Set { name, value } => {
                ipc::send_command(ipc::Command::SetVar { name, value })
            }
            VarSubcommand::Get { name } => ipc::send_command(ipc::Command::GetVar { name }),
            VarSubcommand::Rotate {
                name,
                direction,
                values,
            } => var_rotate(name, direction, values),
            VarSubcommand::List {} => ipc::send_command(ipc::Command::ListVars {}),
        },
    }?;
    if let Some(error) = response.error {
        return Err(anyhow!("{}", error));
    }
    if let Some(response_data) = response.data {
        match response_data {
            ipc::ResponseData::Value(value) => println!("{}", value),
            ipc::ResponseData::Vars(vars) => {
                for (k, v) in vars {
                    println!("{}={}", k, v);
                }
            }
        }
    }
    Ok(())
}
