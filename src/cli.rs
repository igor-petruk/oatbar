use anyhow::anyhow;
use clap::{Parser, Subcommand, ValueEnum};

#[allow(unused)]
mod ipc;

#[derive(Parser)]
#[command(
    author, version,
    about = "A cli tool to interact with oatbar.",
    long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
    /// Unique name of the oatbar server instance.
    #[arg(long, default_value = "oatbar")]
    instance_name: String,
}

#[derive(Clone, ValueEnum)]
enum Direction {
    Right,
    Left,
}

#[derive(Subcommand)]
enum VarSubcommand {
    /// Set a variable value.
    Set {
        /// Variable name.
        name: String,
        /// New variable value.
        value: String,
    },
    /// Get a current variable value.
    Get {
        /// Variable name.
        name: String,
    },
    /// Rotate a variable value through a list of values.
    ///
    /// If a current value is not in the list, the variable is set
    /// to the first value if direction is right, and to the last
    /// value if the direction is left.
    Rotate {
        /// Variable name.
        name: String,
        /// Rotation direction in the list. When going off-limits, appears from the other side.
        direction: Direction,
        /// List of values.
        values: Vec<String>,
    },
    /// List all variables and their values. Useful for troubleshooting.
    #[command(name = "ls")]
    List {},
}

#[derive(Subcommand)]
enum Commands {
    /// Interrupt waiting on all pending command `intervals`,
    /// forcing immediate restart.
    Poke,
    /// Work with oatbar variables.
    Var {
        #[clap(subcommand)]
        var: VarSubcommand,
    },
}

fn var_rotate(
    client: &ipc::Client,
    name: String,
    direction: Direction,
    values: Vec<String>,
) -> anyhow::Result<ipc::Response> {
    if values.is_empty() {
        return Err(anyhow::anyhow!("Values list must be not empty"));
    }
    let response = client.send_command(ipc::Command::GetVar { name: name.clone() })?;
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
    client.send_command(ipc::Command::SetVar {
        name,
        value: new_value.clone(),
    })
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let client = ipc::Client::new(&cli.instance_name)?;
    let response = match cli.command {
        Commands::Poke => client.send_command(ipc::Command::Poke),
        Commands::Var { var } => match var {
            VarSubcommand::Set { name, value } => {
                client.send_command(ipc::Command::SetVar { name, value })
            }
            VarSubcommand::Get { name } => client.send_command(ipc::Command::GetVar { name }),
            VarSubcommand::Rotate {
                name,
                direction,
                values,
            } => var_rotate(&client, name, direction, values),
            VarSubcommand::List {} => client.send_command(ipc::Command::ListVars {}),
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
