# LLM

`oatbar` ships with `oatbar-llm`, a powerful utility that uses LLMs to process data and return it in `i3bar` format.
It can summarize logs, explain errors, fetch news, or just generate cool content.

<!-- toc -->

### How it works

`oatbar-llm` runs the configured `[[command]]`s **before** invoking the LLM. The output of these commands is then fed into the LLM prompt as context. 

This means the LLM **does not** execute tools or commands itself. You have full control over what data is sent to the model for processing.

See [LLM Configuration](../reference/llm.md) for full reference.

### Examples


> [!NOTE]
> These examples are for illustrative purposes. Due to the non-deterministic nature of LLMs, you may need to tune the prompts (questions) to get the exact output format or content you desire for your specific model and use case.

#### System Insight

Analyze system logs and metrics to provide a high-level summary of the system health.

**1. Configure `~/.config/oatbar-llm/config.toml`**

```toml
[llm]
provider="google"
name="gemini-2.5-flash"

[[command]]
cmd="journalctl -p err -n 50 --no-pager"

[[command]]
cmd="df -h"

[[variable]]
name="status"
type="string"
question="Based on the logs and disk usage, what is the system status?"
allowed_answers=["OK", "WARNING", "CRITICAL"]

[[variable]]
name="summary"
type="string"
question="Provide a very brief, one-sentence summary of the system state."
write_to="/tmp/system_summary.md"
```

**2. Configure `oatbar`**

```toml
[[command]]
name="llm"
command="oatbar-llm"
interval=3600

[[block]]
name="llm_status"
type="text"
value="${llm:status.value}"
on_mouse_left="xdg-open /tmp/system_summary.md"
```


#### Smart Git Status

Summarize uncommitted changes in your current project to keep you focused.

**1. Configure `~/.config/oatbar-llm/config.toml`**

```toml
[llm]
provider="google"
name="gemini-2.5-flash"

[[command]]
name="git_status"
cmd="cd ~/Projects/my-project && git status -s && git diff --stat"

[[variable]]
name="git_summary"
type="string"
question="Summarize the uncommitted changes in 3-5 words. If clean, say 'Clean'."
```

**2. Configure `oatbar`**

```toml
[[command]]
name="git_ai"
command="oatbar-llm"
interval=600

[[block]]
name="git_status"
type="text"
value="Git: ${git_ai:git_summary.value}"
```

#### Security Sentinel

Monitor open ports and recent authentication failures for a quick security overview.

**1. Configure `~/.config/oatbar-llm/config.toml`**

```toml
[llm]
provider="google"
name="gemini-2.5-flash"

[[command]]
name="ports"
cmd="ss -tuln"

[[command]]
name="auth_logs"
cmd="journalctl -u sshd -n 20 --no-pager"

[[variable]]
name="security_alert"
type="string"
question="Analyze open ports and sshd logs. Is there any suspicious activity? Answer 'Safe' or 'Suspicious: <reason>'."
```

**2. Configure `oatbar`**

```toml
[[command]]
name="security_ai"
command="oatbar-llm"
interval=3600

[[block]]
name="security"
type="text"
value="Sec: ${security_ai:security_alert.value}"
```

#### Outfit Advisor

Get clothing suggestions based on the current weather.

**1. Configure `~/.config/oatbar-llm/config.toml`**

```toml
[llm]
provider="google"
name="gemini-2.5-flash"

[[command]]
name="weather"
cmd="curl -s 'https://api.open-meteo.com/v1/forecast?latitude=40.76&longitude=-73.99&current=temperature_2m,weather_code'"

[[variable]]
name="outfit"
type="string"
question="Based on this weather JSON (temperature in Celsius), suggest a simple outfit (e.g., 'T-shirt & Shorts', 'Coat & Scarf'). Keep it under 5 words."
```

**2. Configure `oatbar`**

```toml
[[command]]
name="outfit_ai"
command="oatbar-llm"
interval=7200

[[block]]
name="outfit"
type="text"
value="Wear: ${outfit_ai:outfit.value}"
```

#### Process Doctor

Identify resource-hogging processes and suggest actions.

**1. Configure `~/.config/oatbar-llm/config.toml`**

```toml
[llm]
provider="google"
name="gemini-2.5-flash"

[[command]]
name="top_procs"
cmd="ps aux --sort=-%cpu | head -n 6"

[[variable]]
name="proc_analysis"
type="string"
question="Identify the process using the most CPU. Is it normal? Output format: 'High CPU: <process> (<percent>%)'."
```

**2. Configure `oatbar`**

```toml
[[command]]
name="proc_ai"
command="oatbar-llm"
interval=60

[[block]]
name="proc_health"
type="text"
value="${proc_ai:proc_analysis.value}"
```

#### Daily Standup Prep

Summarize your work from the last 24 hours to prepare for your daily standup meeting.

**1. Configure `~/.config/oatbar-llm/config.toml`**

```toml
[llm]
provider="google"
name="gemini-2.5-flash"

[[command]]
name="my_commits"
cmd="cd ~/Projects/my-project && git log --author='My Name' --since='24 hours ago' --oneline"

[[variable]]
name="standup_notes"
type="string"
question="Create a bulleted list of my completed tasks for a standup meeting."
write_to="/tmp/standup_notes.md"
```

**2. Configure `oatbar`**

```toml
[[command]]
name="standup_ai"
command="oatbar-llm"
interval=3600

[[block]]
name="standup"
type="text"
value="Standup Prep"
on_mouse_left="xdg-open /tmp/standup_notes.md"
```

### Knowledge Base Examples

These examples demonstrate how to use the `knowledge_base` feature to provide static context to the LLM, allowing it to act as a specialized assistant.

#### Coding Assistant (Style Guide Enforcer)

Check your code against your team's style guide.

**1. Create `~/.config/oatbar-llm/style_guide.md`**

```markdown
# Team Style Guide
- Prefer `unwrap_or_else` over `unwrap`.
- Use `tracing` for logging, not `println!`.
- All public functions must have documentation.
- Variable names should be descriptive (no `x`, `y`, `temp`).
```

**2. Configure `~/.config/oatbar-llm/config.toml`**

```toml
[llm]
provider="google"
name="gemini-2.5-flash"
knowledge_base="/home/user/.config/oatbar-llm/style_guide.md"

[[command]]
name="git_diff"
cmd="cd ~/Projects/my-project && git diff --cached"

[[variable]]
name="style_review"
type="string"
question="Review the git diff against the style guide. Point out any violations concisely."
```

#### Personal Schedule Assistant

Get reminders based on your personal schedule and priorities.

**1. Create `~/.config/oatbar-llm/schedule.md`**

```markdown
# My Schedule & Priorities
- **Mornings (8am-12pm):** Deep work (Coding, Writing). No meetings.
- **Lunch:** 12pm-1pm.
- **Afternoons (1pm-5pm):** Meetings, Emails, Code Reviews.
- **Evenings:** Learning Rust, Gym.

**Current Focus:** Shipping the LLM module for Oatbar.
```

**2. Configure `~/.config/oatbar-llm/config.toml`**

```toml
[llm]
provider="google"
name="gemini-2.5-flash"
knowledge_base="/home/user/.config/oatbar-llm/schedule.md"

[[command]]
name="current_time"
cmd="date +%H:%M"

[[variable]]
name="focus_tip"
type="string"
question="Based on the current time and my schedule, what should I be focusing on right now? Keep it short."
```

#### Incident Response Helper

Suggest next steps when system errors occur, based on a runbook.

**1. Create `~/.config/oatbar-llm/runbook.md`**

```markdown
# Incident Runbook
- **High CPU:** Check `top`, identify process. If `cargo`, ignore. If unknown, kill.
- **Disk Full:** Clean `/tmp` and `~/.cache`. Check `docker system df`.
- **SSH Failures:** Check `auth.log` for repeated IPs. Ban with `fail2ban`.
- **OOM:** Check kernel logs. Restart service.
```

**2. Configure `~/.config/oatbar-llm/config.toml`**

```toml
[llm]
provider="google"
name="gemini-2.5-flash"
knowledge_base="/home/user/.config/oatbar-llm/runbook.md"

[[command]]
name="sys_errors"
cmd="journalctl -p err -n 10 --no-pager"

[[variable]]
name="incident_action"
type="string"
question="Analyze the recent system errors. Based on the runbook, what is the recommended action?"
```

#### Hacker News Summarizer

Fetch the latest news and get a concise summary on your bar.

**1. Configure `~/.config/oatbar-llm/config.toml`**

```toml
[llm]
provider="google"
name="gemini-2.5-flash"
knowledge_base="/home/user/.config/oatbar-llm/hn_preferences.md"
```

Create `~/.config/oatbar-llm/hn_preferences.md`:

```markdown
I am interested in:
- Rust, Go, C++
- System programming, Linux, Kernel
- AI, LLMs, Machine Learning
- Security, Cryptography

I am NOT interested in:
- Web frameworks (React, Vue, etc.)
```

```toml
[[command]]
name="hn_rss"
cmd="curl -s https://news.ycombinator.com/rss"

[[variable]]
name="top_stories"
type="string"
question="Extract the top 3 most interesting headlines from this RSS feed and combine them into a single, short sentence separated by pipes."
```

**2. Configure `oatbar`**

```toml
[[command]]
name="news"
command="oatbar-llm"
interval=10800 # Every 3 hours

[[block]]
name="news_feed"
type="text"
value="HN: ${news:top_stories.value}"
```

### Tips & Best Practices

#### Debugging Prompts
Before connecting `oatbar-llm` to `oatbar`, run it manually in your terminal to verify the output. Use `oatbar-llm --mode=debug` to see the raw response from the LLM, which is helpful for troubleshooting prompts.

#### Prompt Engineering
LLMs are sensitive to how you ask questions.
- **Be Specific**: Instead of "What's the status?", ask "Summarize the system status in 3 words based on these logs."
- **Define Output**: Explicitly state the desired format (e.g., "Format: ...").
- **Iterate**: Use the debug mode to tweak your prompt until you get consistent results.

#### Quota Management
LLM API calls can be expensive or rate-limited.
- **Watch your usage**: Monitor your provider's dashboard.
- **Increase Intervals**: For non-critical data (like weather or news), set the `interval` in `oatbar` to a higher value (e.g., `3600` for 1 hour, `10800` for 3 hours).

#### Consolidating Queries
To save on API calls and context window usage, combine related tasks into a single `oatbar-llm` configuration.
Instead of having one config for "CPU" and another for "Memory", fetch both metrics in the `[[command]]` section and ask for a combined summary populating multiple `[[variable]]`s.

