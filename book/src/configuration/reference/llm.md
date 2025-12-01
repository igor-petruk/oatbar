# LLM Configuration

`oatbar-llm` is configured via `~/.config/oatbar-llm/config.toml`.

## Structure

The configuration file consists of three main sections:
1.  `[llm]`: Global LLM provider settings.
2.  `[[command]]`: External commands to gather context.
3.  `[[variable]]`: Variables to extract or generate using the LLM.

## `[llm]` Section

Configures the LLM provider and global behavior.

| Field | Type | Default | Description |
|---|---|---|---|
| `provider` | string | **Required** | The LLM provider. Supported: `google`, `openai`, `anthropic`, `mistral`, `xai`, `ollama`. |
| `name` | string | **Required** | The model name (e.g., `gemini-2.5-flash`, `gpt-4o`). |
| `role` | string | *Default system prompt* | Custom system prompt to define the AI's persona and goal. |
| `temperature` | float | `0.6` | Controls randomness (0.0 = deterministic, 1.0 = creative). |
| `max_tokens` | int | `3000` | Maximum number of tokens in the response. |
| `url` | string | `None` | Custom API URL (useful for local LLMs or proxies). |
| `knowledge_base` | path | `None` | Path to a text file containing static context/preferences to include in the prompt. |
| `output_format_prompt` | string | `None` | Custom instruction for output format (required if using `Custom` output mode). |
| `retries` | int | `5` | Number of retries for failed API calls. |
| `back_off` | duration | `1s` | Initial backoff duration for retries. |

## `[[command]]` Section

Defines shell commands to run. Their output is fed to the LLM as context.

| Field | Type | Default | Description |
|---|---|---|---|
| `cmd` | string | **Required** | The shell command to execute. |
| `name` | string | `cmd` string | A unique name to refer to this command's output in the prompt context. |

## `[[variable]]` Section

Defines the questions to ask the LLM and how to handle the answers.

| Field | Type | Default | Description |
|---|---|---|---|
| `name` | string | **Required** | The key for the variable in the output JSON. |
| `question` | string | **Required** | The prompt/question for the LLM to answer to populate this variable. |
| `type` | string | `string` | The expected data type: `string`, `number`, `boolean`. |
| `allowed_answers` | list | `None` | A list of valid string values (enum) to restrict the output. |
| `max_length` | int | `None` | Maximum length of the string response. |
| `write_to` | path | `None` | If set, the variable's value will be written to this file. |

## Output Modes

`oatbar-llm` supports different output modes via the `--mode` CLI flag:

-   `json` (default): Outputs a JSON object suitable for `oatbar` (i3bar format).
-   `debug`: Prints the full prompt and raw response for debugging.
-   `custom`: Outputs raw text based on `output_format_prompt`. Useful for generating reports or files.

## Configuring Keys

API keys are **not** stored in the configuration file. Instead, `oatbar-llm` reads them from specific files in the configuration directory (`~/.config/oatbar-llm/`).

| Provider | Key File Path |
|---|---|
| **Google** | `~/.config/oatbar-llm/google_api_key` |
| **OpenAI** | `~/.config/oatbar-llm/openai_api_key` |
| **Anthropic** | `~/.config/oatbar-llm/anthropic_api_key` |
| **Mistral** | `~/.config/oatbar-llm/mistral_api_key` |
| **xAI** | `~/.config/oatbar-llm/xai_api_key` |
| **Ollama** | *Not required* |

Ensure these files contain **only** the API key (no newlines or extra spaces preferred, though whitespace is trimmed).

### Ollama Configuration

Ollama does not require an API key. However, you may need to specify the URL if it's not running on the default port.

```toml
[llm]
provider="ollama"
name="llama3"
url="http://localhost:11434" # Optional, defaults to this value
```

## CLI Options

-   `--config <FILE>`: Path to a custom config file (default: `~/.config/oatbar-llm/config.toml`).
-   `--mode <MODE>`: Output mode (`json`, `debug`, `custom`).

