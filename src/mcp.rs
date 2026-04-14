use anyhow::Result;
use clap::Parser;
use rmcp::{
    handler::server::router::tool::ToolRouter,
    handler::server::wrapper::Parameters,
    model::{ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router,
    transport::stdio,
    ServerHandler, ServiceExt,
};
use tracing_subscriber::{self, EnvFilter};

#[allow(unused)]
mod ipc;
#[allow(unused)]
mod process;
#[allow(unused)]
mod restart;

#[derive(Parser, Debug, Clone)]
#[command(
    author, version,
    about = "A Model Context Protocol server exposing oatbar IPC commands.",
    long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    /// Unique name of the oatbar server instance.
    #[arg(long, default_value = "oatbar")]
    instance_name: String,
}

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct Context {
    #[schemars(
        description = "Optional explanation of why you are issuing this command. Recommended to set for logging."
    )]
    pub human_description: Option<String>,
    #[schemars(
        description = "Name of the MCP client/agent issuing this command (e.g. 'Gemini CLI', 'Claude Desktop'). Clients MUST set this so the user can see which agent is active on their status bar."
    )]
    pub agent_name: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct DumpSvgRequest {
    #[serde(flatten)]
    pub context: Context,
    #[schemars(description = "Absolute path to write the SVG file to (e.g. /tmp/bar.svg).")]
    pub path: String,
    #[schemars(description = "Index of the bar to dump (usually 0).")]
    pub index: usize,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct PokeRequest {
    #[serde(flatten)]
    pub context: Context,
    #[schemars(description = "Name of the command to poke. If not specified, pokes all commands.")]
    pub command: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SetVarRequest {
    #[serde(flatten)]
    pub context: Context,
    #[schemars(description = "Variable name.")]
    pub name: String,
    #[schemars(description = "New variable value.")]
    pub value: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct GetVarRequest {
    #[serde(flatten)]
    pub context: Context,
    #[schemars(description = "Variable name.")]
    pub name: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ListVarsRequest {
    #[serde(flatten)]
    pub context: Context,
    #[schemars(description = "Optional regex to filter variable names.")]
    pub filter: Option<String>,
    #[schemars(
        description = "If true, returns only the names of the variables without their values. Useful to explore what data is available while saving LLM tokens. Some variable values like icon pixmaps may be very long."
    )]
    pub names_only: Option<bool>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ReportStatusRequest {
    #[serde(flatten)]
    pub context: Context,
    #[schemars(
        description = "Short text describing the status of the MCP operation (e.g. 'Value foo set')."
    )]
    pub status: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct RestartRequest {
    #[serde(flatten)]
    pub context: Context,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct McpConfig {
    pub hidden_variables: Vec<String>,
    pub recent_timeout_seconds: u64,
    pub mcp_name: Option<String>,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            hidden_variables: vec![],
            recent_timeout_seconds: 5,
            mcp_name: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConfigContainer {
    pub config: McpConfig,
    pub hidden_regexes: Vec<regex::Regex>,
}

impl ConfigContainer {
    fn is_hidden(&self, name: &str) -> bool {
        self.hidden_regexes.iter().any(|re| re.is_match(name))
    }
}

fn load_config() -> Result<ConfigContainer> {
    use std::path::PathBuf;

    let mut config_path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("~/.config"));
    config_path.push("oatbar/mcp.toml");

    let config: McpConfig = match std::fs::read_to_string(&config_path) {
        Ok(contents) => toml::from_str(&contents)
            .map_err(|e| anyhow::anyhow!("Failed to parse config at {:?}: {}", config_path, e))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => McpConfig::default(),
        Err(e) => {
            return Err(anyhow::anyhow!(
                "Failed to read config at {:?}: {}",
                config_path,
                e
            ))
        }
    };

    let hidden_regexes = config
        .hidden_variables
        .iter()
        .map(|s| regex::Regex::new(s))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| anyhow::anyhow!("Failed to compile rule in 'hidden_variables': {}", e))?;

    Ok(ConfigContainer {
        config,
        hidden_regexes,
    })
}

#[derive(Debug, Clone)]
pub struct OatbarMcp {
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
    instance_name: String,
    last_status_seq: std::sync::Arc<std::sync::atomic::AtomicU64>,
    config: std::sync::Arc<ConfigContainer>,
}

#[tool_router]
impl OatbarMcp {
    pub fn new(instance_name: String, config: ConfigContainer) -> Self {
        Self {
            tool_router: Self::tool_router(),
            instance_name,
            last_status_seq: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
            config: std::sync::Arc::new(config),
        }
    }

    fn client(&self) -> Result<ipc::Client, String> {
        ipc::Client::new(&self.instance_name).map_err(|e| e.to_string())
    }

    fn var_name(&self, base: &str) -> String {
        if let Some(ref name) = self.config.config.mcp_name {
            format!("mcp:{}.{}", name, base)
        } else {
            format!("mcp:{}", base)
        }
    }

    fn report_status_internal(&self, status: String, agent_name: Option<String>) {
        let Ok(client) = self.client() else {
            return;
        };

        let value_var = self.var_name("value");
        let _ = client.send_command(ipc::Command::SetVar {
            name: value_var,
            value: status,
        });

        if let Some(agent) = agent_name {
            let agent_var = self.var_name("agent");
            let _ = client.send_command(ipc::Command::SetVar {
                name: agent_var,
                value: agent,
            });
        }

        let recent_var = self.var_name("recent");
        let _ = client.send_command(ipc::Command::SetVar {
            name: recent_var.clone(),
            value: "1".into(),
        });

        let seq = self
            .last_status_seq
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        let last_status_seq = self.last_status_seq.clone();
        let instance_name = self.instance_name.clone();

        let timeout = self.config.config.recent_timeout_seconds;
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_secs(timeout)).await;
            if last_status_seq.load(std::sync::atomic::Ordering::SeqCst) == seq {
                let _ = tokio::task::spawn_blocking(move || {
                    if let Ok(client) = ipc::Client::new(&instance_name) {
                        let _ = client.send_command(ipc::Command::SetVar {
                            name: recent_var,
                            value: "".into(),
                        });
                    }
                })
                .await;
            }
        });
    }

    #[tool(
        description = "Dump the current bar state to an absolute SVG file path (e.g., /tmp/bar.svg). Useful for debugging layout or rendering issues. Note: If you just restarted oatbar or changed config, wait a few seconds before calling this to ensure variables have propagated and the state is stable."
    )]
    fn dump_svg(&self, Parameters(req): Parameters<DumpSvgRequest>) -> Result<String, String> {
        if let Some(desc) = req.context.human_description {
            self.report_status_internal(desc, req.context.agent_name.clone());
        }
        let response = self
            .client()?
            .send_command(ipc::Command::DumpSvg {
                path: req.path,
                index: req.index,
            })
            .map_err(|e| e.to_string())?;
        if let Some(error) = response.error {
            return Err(error);
        }
        Ok("SVG dumped successfully".into())
    }

    #[tool(
        description = "Interrupt waiting on all pending command `intervals`, forcing immediate restart."
    )]
    fn poke(&self, Parameters(req): Parameters<PokeRequest>) -> Result<String, String> {
        if let Some(desc) = req.context.human_description {
            self.report_status_internal(desc, req.context.agent_name);
        }
        let response = self
            .client()?
            .send_command(ipc::Command::Poke { name: req.command })
            .map_err(|e| e.to_string())?;
        if let Some(error) = response.error {
            return Err(error);
        }
        Ok("ok".to_string())
    }

    #[tool(description = "Set a variable value.")]
    fn set_var(&self, Parameters(req): Parameters<SetVarRequest>) -> Result<String, String> {
        if let Some(desc) = req.context.human_description {
            self.report_status_internal(desc, req.context.agent_name);
        }
        if self.config.is_hidden(&req.name) {
            return Err("Access to this variable is restricted by MCP security settings.".into());
        }
        let response = self
            .client()?
            .send_command(ipc::Command::SetVar {
                name: req.name,
                value: req.value,
            })
            .map_err(|e| e.to_string())?;
        if let Some(error) = response.error {
            return Err(error);
        }
        Ok("ok".to_string())
    }

    #[tool(description = "Get a current variable value.")]
    fn get_var(&self, Parameters(req): Parameters<GetVarRequest>) -> Result<String, String> {
        if let Some(desc) = req.context.human_description {
            self.report_status_internal(desc, req.context.agent_name);
        }
        if self.config.is_hidden(&req.name) {
            return Err("Access to this variable is restricted by MCP security settings.".into());
        }
        let response = self
            .client()?
            .send_command(ipc::Command::GetVar { name: req.name })
            .map_err(|e| e.to_string())?;
        if let Some(error) = response.error {
            return Err(error);
        }
        match response.data {
            Some(ipc::ResponseData::Value(value)) => Ok(value),
            _ => Err("Unexpected response type".into()),
        }
    }

    #[tool(
        description = "List all variables and their values. Useful for troubleshooting and exploring available context. Note that oatbar variables are completely dynamic; new variables can appear later at runtime as system states change."
    )]
    fn list_vars(&self, Parameters(req): Parameters<ListVarsRequest>) -> Result<String, String> {
        if let Some(desc) = req.context.human_description {
            self.report_status_internal(desc, req.context.agent_name);
        }
        let filter_regex = match req.filter {
            Some(ref f) => Some(regex::Regex::new(f).map_err(|e| format!("Invalid regex: {}", e))?),
            None => None,
        };

        let response = self
            .client()?
            .send_command(ipc::Command::ListVars {})
            .map_err(|e| e.to_string())?;
        if let Some(error) = response.error {
            return Err(error);
        }
        match response.data {
            Some(ipc::ResponseData::Vars(mut vars)) => {
                vars.retain(|k, _| !self.config.is_hidden(k));

                if let Some(re) = filter_regex {
                    vars.retain(|k, _| re.is_match(k));
                }

                if req.names_only.unwrap_or(false) {
                    let keys: Vec<String> = vars.into_keys().collect();
                    Ok(serde_json::to_string_pretty(&keys).map_err(|e| e.to_string())?)
                } else {
                    Ok(serde_json::to_string_pretty(&vars).map_err(|e| e.to_string())?)
                }
            }
            _ => Err("Unexpected response type".into()),
        }
    }

    #[tool(
        description = "Report an arbitrary custom status message natively on the user's status bar! Use this when the user peculiarly commands you to display a distinctive message, or on demand."
    )]
    fn report_status(
        &self,
        Parameters(req): Parameters<ReportStatusRequest>,
    ) -> Result<String, String> {
        self.report_status_internal(req.status, req.context.agent_name);
        Ok("ok".to_string())
    }

    #[tool(
        description = "Restart oatbar securely via IPC. Useful for applying layout or config file changes seamlessly. It dynamically loads the state entirely avoiding console disruptions."
    )]
    fn restart_oatbar(
        &self,
        Parameters(req): Parameters<RestartRequest>,
    ) -> Result<String, String> {
        if let Some(desc) = req.context.human_description {
            self.report_status_internal(desc, req.context.agent_name);
        }

        crate::restart::restart_oatbar(&self.instance_name).map_err(|e| e.to_string())?;
        Ok("oatbar restarted cleanly!".to_string())
    }
}

#[tool_handler]
impl ServerHandler for OatbarMcp {
    fn get_info(&self) -> ServerInfo {
        let mcp_vars = match &self.config.config.mcp_name {
            Some(name) => format!("'mcp:{}.*'", name),
            None => "'mcp:*'".to_string(),
        };

        let instructions =
            include_str!("../data/mcp_instructions.md").replace("{mcp_vars}", &mcp_vars);

        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(instructions)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    tracing::info!(
        "Starting oatbar MCP server for instance: {}",
        cli.instance_name
    );

    let config = load_config()?;

    let service = OatbarMcp::new(cli.instance_name, config)
        .serve(stdio())
        .await
        .inspect_err(|e| {
            tracing::error!("serving error: {:?}", e);
        })?;

    service.waiting().await?;
    Ok(())
}
