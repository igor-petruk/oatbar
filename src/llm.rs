mod protocol;

use anyhow::{anyhow, Context};
use chrono::{DateTime, Local, TimeZone};
use clap::Parser;
use llm;
use protocol::i3bar;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, HashMap};
use std::process::Stdio;
use std::{io::Read, path::PathBuf};
use tracing::debug;

const DATA_INPUT_FORMAT: &str = r#"
# System Role

# Data Input Format
I will provide the output of one or more Unix commands below enclosed in XML tags.
- The `<cmd>` tag contains the exact command executed.
  - The `timestamp` attribute contains the exact time the command was executed.
  - The `exit_code` attribute contains the exit code of the command.
  - The `name` attribute of the `<cmd>` tag contains the name of the command to be referred later.
- The `<stdout>` tag contains the unescaped, raw text returned by the shell.
"#;

#[derive(Debug, Deserialize)]
pub struct Model {
    provider: String,
    name: String,
    role: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Command {
    name: Option<String>,
    cmd: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
#[serde(tag = "type")]
pub enum VariableKind {
    String {
        allowed_answers: Option<Vec<String>>,
        max_length: Option<usize>,
    },
    Boolean,
    Number,
}

#[derive(Debug, Deserialize)]
pub struct Variable {
    name: String,
    question: String,
    #[serde(flatten)]
    kind: VariableKind,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    model: Model,
    #[serde(rename = "command", default)]
    commands: Vec<Command>,
    #[serde(rename = "variable", default)]
    variables: Vec<Variable>,
}

#[derive(Debug, Serialize)]
pub struct RunResult {
    stdout: String,
    exit_code: i32,
    timestamp: u64,
}

pub fn run_commands(commands: &[Command]) -> anyhow::Result<HashMap<String, RunResult>> {
    let mut results = HashMap::new();
    for cmd in commands {
        let output = std::process::Command::new("sh")
            .arg("-c")
            .arg(&cmd.cmd)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .output()?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let exit_code = output
            .status
            .code()
            .ok_or_else(|| anyhow!("Process terminated by signal"))?;
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let name = cmd.name.clone().unwrap_or_else(|| cmd.cmd.clone());
        results.insert(
            name,
            RunResult {
                stdout,
                exit_code,
                timestamp,
            },
        );
    }
    Ok(results)
}

pub fn load(config_path: &Option<PathBuf>) -> anyhow::Result<Config> {
    let path = if let Some(config_path) = config_path {
        config_path.clone()
    } else {
        let mut path = dirs::config_dir().context("Missing config dir")?;
        path.push("oatbar-llm.toml");
        path
    };
    if !path.exists() {
        return Err(anyhow!("Config file {:?} does not exist", path));
    }
    let mut file = std::fs::File::open(&path).context(format!("unable to open {:?}", &path))?;
    let mut data = String::new();
    file.read_to_string(&mut data)?;

    let config: Config = toml::from_str(&data)?;
    debug!("Parsed config:\n{:#?}", config);
    Ok(config)
}

#[derive(Parser)]
#[command(
    author, version,
    about = "LLM util for oatbar",
    long_about = None)]
#[command(propagate_version = true)]
#[derive(Debug)]
struct Cli {
    #[clap(short, long)]
    config: Option<PathBuf>,
}

fn generate_schema(variables: &[Variable]) -> anyhow::Result<llm::chat::StructuredOutputFormat> {
    let mut properties = serde_json::Map::new();
    let mut required = vec![];

    for variable in variables {
        let value_schema = match &variable.kind {
            VariableKind::String {
                allowed_answers,
                max_length,
            } => {
                if let Some(answers) = allowed_answers {
                    json!({ "type": "string", "enum": answers })
                } else if let Some(max_len) = max_length {
                    json!({ "type": "string", "maxLength": max_len })
                } else {
                    json!({ "type": "string" })
                }
            }
            VariableKind::Boolean => json!({ "type": "boolean" }),
            VariableKind::Number => json!({ "type": "number" }),
        };
        properties.insert(variable.name.clone(), value_schema);
        required.push(variable.name.clone());
    }

    let schema = json!({
        "name": "Variables",
        "schema": {
        "type": "object",
           "properties": properties,
           "required": required
        }
    });

    Ok(serde_json::from_value(schema)?)
}

fn generate_prompt(config: &Config, comman_results: &HashMap<String, RunResult>) -> String {
    let mut prompt = String::new();
    prompt.push_str("# Role\n");
    if let Some(role) = &config.model.role {
        prompt.push_str(&format!("{}\n", role));
    } else {
        prompt.push_str(
            r#"You are an expert Linux System Administrator and DevOps Engineer.
Your goal is to analyze raw command line output, identify anomalies,
track historical changes, and provide actionable conclusions.
"#,
        );
    }
    prompt.push_str("\n# Data Input Format\n");
    prompt.push_str(DATA_INPUT_FORMAT);
    prompt.push_str("\n# Command Outputs\n");
    for (name, result) in comman_results {
        let dt: DateTime<Local> = Local.timestamp_opt(result.timestamp as i64, 0).unwrap();
        prompt.push_str("```\n");
        prompt.push_str(&format!(
            "<cmd name=\"{}\" timestamp=\"{}\" exit_code=\"{}\">\n<output>\n{}</output>\n</cmd>\n\n",
            name,
            dt.format("%Y-%m-%d %H:%M:%S %Z"),
            result.exit_code,
            result.stdout
        ));
        prompt.push_str("```\n");
    }

    prompt.push_str("\n# Variables with questions to answer\n");
    prompt.push_str("Answer each question below populating the variable with the answer.\n\n");
    for variable in &config.variables {
        prompt.push_str(&format!("## {}\n", variable.name));
        prompt.push_str(&format!("* **Question:** {}\n", variable.question));
        prompt.push_str("* **Allowed answers:** ");
        match &variable.kind {
            VariableKind::String {
                allowed_answers,
                max_length,
            } => {
                if let Some(answers) = allowed_answers {
                    prompt.push_str(&format!("{:?} without quotes", answers));
                } else {
                    prompt.push_str("any string");
                    if let Some(max_len) = max_length {
                        prompt.push_str(&format!(" (max length: {})", max_len));
                    }
                }
                prompt.push_str("\n");
            }
            VariableKind::Boolean => {
                prompt.push_str("true or false\n");
            }
            VariableKind::Number => {
                prompt.push_str("any number\n");
            }
        }
    }
    prompt
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let sub = tracing_subscriber::fmt().compact().with_thread_names(true);
    #[cfg(debug_assertions)]
    let sub = sub.with_max_level(tracing::Level::TRACE);
    sub.init();

    let cli = Cli::parse();

    debug!("Parsed command line: {:#?}", cli);

    let config = load(&cli.config)?;

    let command_result = run_commands(&config.commands).context("Failed to run commands")?;

    let prompt = generate_prompt(&config, &command_result);
    debug!("Prompt:\n{}", prompt);

    let schema = generate_schema(&config.variables).context("Failed to generate schema")?;
    debug!("Schema:\n{:#?}", schema);

    let api_key = std::env::var("GOOGLE_API_KEY")?;
    let llm = llm::builder::LLMBuilder::new()
        .backend(config.model.provider.parse().context("Invalid backend")?)
        .model(&config.model.name)
        .api_key(api_key)
        .schema(schema)
        .max_tokens(1000)
        .temperature(0.5)
        .build()?;

    let mut messages = vec![];
    messages.push(llm::chat::ChatMessage::user().content(&prompt).build());

    let response = llm.chat(&messages).await?;
    debug!("Response: {:#?}", response);

    let response_json: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&response.text().expect("Failed to get response text"))
            .context("Failed to parse response")?;
    debug!("Response JSON: {:#?}", response_json);

    println!("{}", serde_json::to_string(&i3bar::Header::default())?);
    println!("[");

    let mut blocks = vec![];
    for (key, value) in response_json {
        let full_text = format!("{}: {}", key, value.to_string());
        let mut others = BTreeMap::new();
        others.insert("value".to_string(), value);
        blocks.push(i3bar::Block {
            full_text,
            name: Some(key),
            instance: None,
            other: others,
        });
    }
    println!("{},", serde_json::to_string(&blocks)?);
    println!("]");

    Ok(())
}
