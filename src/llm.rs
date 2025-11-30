mod protocol;

use anyhow::{anyhow, Context};
use chrono::{DateTime, Local, TimeZone};
use clap::{Parser, ValueEnum};
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

#[derive(Debug, Clone, Copy, ValueEnum, Default, PartialEq)]
enum OutputMode {
    #[default]
    Json,
    Debug,
    Custom,
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
    #[clap(short, long, default_value = "json")]
    mode: OutputMode,
}

#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
enum SchemaMode {
    Off,
    #[default]
    Auto,
}

#[derive(Debug, Deserialize)]
pub struct LLM {
    provider: String,
    name: String,
    role: Option<String>,
    temperature: Option<f32>,
    max_tokens: Option<usize>,
    url: Option<String>,
    retries: Option<usize>,
    #[serde(default, with = "serde_ext_duration::opt")]
    back_off: Option<std::time::Duration>,
    #[serde(default, with = "serde_ext_duration::opt")]
    max_back_off: Option<std::time::Duration>,
    output_format_prompt: Option<String>,
    #[serde(default)]
    schema_mode: SchemaMode,
    schema: Option<String>,
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
    write_to: Option<PathBuf>,
    #[serde(flatten)]
    kind: VariableKind,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde()]
    llm: LLM,
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
        path.push("oatbar-llm/config.toml");
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

fn generate_schema(variables: &[Variable]) -> anyhow::Result<llm::chat::StructuredOutputFormat> {
    let mut properties = serde_json::Map::new();
    let mut required = vec![];

    for variable in variables {
        let mut schema = variable.kind.to_schema();
        if let Some(obj) = schema.as_object_mut() {
            obj.insert("description".to_string(), json!(variable.question));
        }
        properties.insert(variable.name.clone(), schema);
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
    cli: &Cli,
    config: &Config,
    comman_results: &HashMap<String, RunResult>,
) -> anyhow::Result<String> {
    let mut prompt = String::new();
    writeln!(prompt, "# Role")?;
    if let Some(role) = &config.llm.role {
        writeln!(prompt, "{}", role)?;
    } else {
        writeln!(
            prompt,
            r#"You are an expert Linux System Administrator and DevOps Engineer.
Your goal is to analyze raw command line output, identify anomalies,
track historical changes, and provide actionable conclusions."#
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
    match cli.mode {
        OutputMode::Debug => {
            writeln!(
                prompt,
                "This is debug mode. You must explain your reasoning in plain text."
            )?;
            writeln!(
                prompt,
                "After the explanation include a section with the variable values that you have chosen."
            )?;
        }
        OutputMode::Json => {
            writeln!(
                prompt,
                "You must output ONLY a valid JSON object without any suffix or prefix."
            )?;
            writeln!(
                prompt,
                "Especially no wrapping in Markdown or any other format."
            )?;
        }
        OutputMode::Custom => {
            if let Some(format_prompt) = &config.llm.output_format_prompt {
                writeln!(prompt, "{}", format_prompt)?;
            } else {
                return Err(anyhow!("output_format_prompt is required for custom mode"));
            }
        }
    }

    if !config.variables.is_empty() {
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
    }
    Ok(prompt)
}

fn write_variables_to_files(response_text: &str, variables: &[Variable]) -> anyhow::Result<()> {
    if variables.is_empty() {
        return Ok(());
    }

    let response_json: serde_json::Map<String, serde_json::Value> =
        match serde_json::from_str(response_text) {
            Ok(json) => json,
            Err(_) => {
                debug!("Failed to parse response as JSON, skipping file writing");
                return Ok(());
            }
        };

    for variable in variables {
        if let Some(path) = &variable.write_to {
            if let Some(value) = response_json.get(&variable.name) {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).context("Failed to create parent dir")?;
                }
                let value_str = if let Some(s) = value.as_str() {
                    s.to_string()
                } else {
                    value.to_string()
                };
                std::fs::write(path, value_str).context("Failed to write to file")?;
            }
        }
    }
    Ok(())
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

    let prompt = generate_prompt(&cli, &config, &command_result)?;
    debug!("Prompt:\n{}", prompt);

    let mut builder = llm::builder::LLMBuilder::new()
        .backend(config.llm.provider.parse().context("Invalid backend")?)
        .model(&config.llm.name);

    let schema_mode = if cli.mode == OutputMode::Debug {
        SchemaMode::Off
    } else if config.llm.schema_mode == SchemaMode::Off && cli.mode == OutputMode::Json {
        SchemaMode::Auto
    } else {
        config.llm.schema_mode
    };

    debug!("Schema mode: {:#?}", schema_mode);
    if schema_mode == SchemaMode::Auto {
        let schema = if let Some(schema_str) = config.llm.schema.clone() {
            serde_json::from_str(&schema_str).context("Failed to parse schema")?
        } else {
            schema
        };
        debug!("Schema:\n{:#?}", schema);
        builder = builder.schema(schema);
    };

    let mut builder = builder
        .resilient(true)
        .resilient_attempts(config.llm.retries.unwrap_or(5))
        .resilient_backoff(
            config
                .llm
                .back_off
                .map(|d| d.as_millis() as u64)
                .unwrap_or(1000),
            config
                .llm
                .max_back_off
                .map(|d| d.as_millis() as u64)
                .unwrap_or(5000),
        )
        .max_tokens(config.llm.max_tokens.unwrap_or(3000) as u32)
        .temperature(config.llm.temperature.unwrap_or(0.9))
        .validator_attempts(config.llm.retries.unwrap_or(5))
        .validator(|text| {
            if text.is_empty() {
                Err("Response is empty".to_string())
            } else {
                Ok(())
            }
        });

    if config.llm.provider == "ollama" {
        let url = config
            .llm
            .url
            .unwrap_or_else(|| "http://127.0.0.1:11434".to_string());
        builder = builder.base_url(&url).api_key("");
    } else {
        let mut key_path = dirs::config_dir().context("Missing config dir")?;
        key_path.push("oatbar-llm");
        key_path.push(format!("{}_api_key", config.llm.provider));

        let api_key = std::fs::read_to_string(&key_path)
            .context(format!("Failed to read api key from {:?}", key_path))?;
        builder = builder.api_key(api_key.trim());
        if let Some(url) = &config.llm.url {
            builder = builder.base_url(url);
        }
    }

    let llm = builder.build()?;

    let messages = vec![llm::chat::ChatMessage::user().content(&prompt).build()];

    let response = llm.chat(&messages).await?;
    debug!("Response: {:#?}", response);

    let response_text = response.text().context("Failed to get response text")?;
    write_variables_to_files(&response_text, &config.variables)?;

    if cli.mode == OutputMode::Debug {
        println!("--------------------- Prompt ------------------------");
        println!("{}", prompt);
        println!("--------------------- Response ----------------------");
        println!("{}", response_text);
    } else if cli.mode == OutputMode::Custom {
        println!("{}", response_text);
    } else {
        print_i3bar_output(&response_text)?;
    }

    Ok(())
}
