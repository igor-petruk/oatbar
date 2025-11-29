mod protocol;

use anyhow::{anyhow, Context};
use chrono::{DateTime, Local, TimeZone};
use clap::Parser;
use protocol::i3bar;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, HashMap};
use std::process::Stdio;
use std::{fmt::Write, io::Read, path::PathBuf};
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
    temperature: Option<f32>,
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

impl VariableKind {
    pub fn to_schema(&self) -> serde_json::Value {
        match self {
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
        }
    }

    pub fn describe_allowed_answers(&self) -> String {
        match self {
            VariableKind::String {
                allowed_answers,
                max_length,
            } => {
                let mut description = String::new();
                if let Some(answers) = allowed_answers {
                    description.push_str(&format!("{:?} without quotes", answers));
                } else {
                    description.push_str("any string");
                    if let Some(max_len) = max_length {
                        description.push_str(&format!(" (max length: {})", max_len));
                    }
                }
                description
            }
            VariableKind::Boolean => "true or false".to_string(),
            VariableKind::Number => "any number".to_string(),
        }
    }
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
        properties.insert(variable.name.clone(), variable.kind.to_schema());
        required.push(variable.name.clone());
    }

    let schema = json!({
        "name": "Variables",
        "schema": {
        "type": "object",
           "properties": properties,
           "required": required
        },
        "strict": false
    });

    Ok(serde_json::from_value(schema)?)
}

fn generate_prompt(
    config: &Config,
    comman_results: &HashMap<String, RunResult>,
) -> anyhow::Result<String> {
    let mut prompt = String::new();
    writeln!(prompt, "# Role")?;
    if let Some(role) = &config.model.role {
        writeln!(prompt, "{}", role)?;
    } else {
        writeln!(
            prompt,
            r#"You are an expert Linux System Administrator and DevOps Engineer.
Your goal is to analyze raw command line output, identify anomalies,
track historical changes, and provide actionable conclusions.
"#
        )?;
    }
    writeln!(prompt, "\n# Data Input Format")?;
    write!(prompt, "{}", DATA_INPUT_FORMAT)?;
    writeln!(prompt, "\n# Command Outputs")?;
    for (name, result) in comman_results {
        let dt: DateTime<Local> = Local.timestamp_opt(result.timestamp as i64, 0).unwrap();
        writeln!(prompt, "```")?;
        writeln!(
            prompt,
            "<cmd name=\"{}\" timestamp=\"{}\" exit_code=\"{}\">\n<output>\n{}</output>\n</cmd>\n",
            name,
            dt.format("%Y-%m-%d %H:%M:%S %Z"),
            result.exit_code,
            result.stdout
        )?;
        writeln!(prompt, "```")?;
    }

    writeln!(prompt, "\n# Output Format")?;
    writeln!(
        prompt,
        "You must output ONLY a valid JSON object without any suffix or prefix."
    )?;
    writeln!(
        prompt,
        "Especially no wrapping in Markdown or any other format."
    )?;

    writeln!(prompt, "\n# Variables with questions to answer")?;
    writeln!(
        prompt,
        "Answer each question below populating the variable with the answer.\n"
    )?;
    for variable in &config.variables {
        writeln!(prompt, "## {}", variable.name)?;
        writeln!(prompt, "* **Question:** {}", variable.question)?;
        writeln!(
            prompt,
            "* **Allowed answers:** {}",
            variable.kind.describe_allowed_answers()
        )?;
    }
    Ok(prompt)
}

fn print_i3bar_output(response_text: &str) -> anyhow::Result<()> {
    let response_json: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(response_text).context("Failed to parse response")?;
    debug!("Response JSON: {:#?}", response_json);

    println!("{}", serde_json::to_string(&i3bar::Header::default())?);
    println!("[");

    let mut blocks = vec![];
    for (key, value) in response_json {
        let value_str = if let Some(s) = value.as_str() {
            s.to_string()
        } else {
            value.to_string()
        };
        let full_text = format!("{}: {}", key, value_str);
        let mut others = BTreeMap::new();
        others.insert("value".to_string(), value);
        blocks.push(i3bar::Block {
            full_text,
            name: Some(key),
            instance: None,
            other: others,
        });
    }
    println!("{}", serde_json::to_string(&blocks)?);
    println!("]");
    Ok(())
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

    let schema = generate_schema(&config.variables).context("Failed to generate schema")?;
    debug!("Schema:\n{:#?}", schema);

    let prompt = generate_prompt(&config, &command_result)?;
    debug!("Prompt:\n{}", prompt);

    let mut key_path = dirs::config_dir().context("Missing config dir")?;
    key_path.push("oatbar");
    key_path.push(format!("{}_api_key", config.model.provider));

    let api_key = std::fs::read_to_string(&key_path)
        .context(format!("Failed to read api key from {:?}", key_path))?;
    let llm = llm::builder::LLMBuilder::new()
        .backend(config.model.provider.parse().context("Invalid backend")?)
        .model(&config.model.name)
        .api_key(api_key.trim())
        .schema(schema)
        .max_tokens(1000)
        .temperature(config.model.temperature.unwrap_or(0.5))
        .build()?;

    let messages = vec![llm::chat::ChatMessage::user().content(&prompt).build()];

    let response = llm.chat(&messages).await?;
    debug!("Response: {:#?}", response);

    print_i3bar_output(&response.text().context("Failed to get response text")?)?;

    Ok(())
}
