# Oatbar 0.3.0 (Unreleased)

This update introduces several major new features and reliability improvements, expanding Oatbar's reach to Wayland and enhancing its interaction with AI agents.

## Major Features

- **Wayland Support**: Full support for Wayland compositors via `smithay-client-toolkit`. Oatbar now runs natively on modern displays.
- **Tray Support (SNI)**: Integration with the **Status Notifier Item (SNI)** protocol. Display and interact with system tray icons from your status bar.
- **MCP Server Integration**: A powerful new **Model Context Protocol (MCP)** server allows AI agents to interact directly with your status bar, reporting status and setting variables. You can configure
your bar with the help of MCP because you agent has access to configs and data.
- **MPRIS Support**: Control your music and media players directly from Oatbar. View metadata like artist and track titles in real-time.
- **Battery Module**: Built-in support for monitoring battery status, including charging states and percentages.
- **Improved Controls**: `oatctl poke` now supports targeting specific commands, making it easier to trigger updates for individual modules.

## Smaller Fixes & Improvements

- **Monitor Compatibility**: Fixed issues with monitor matching and vertical offsets (`monitor.y`) to ensure correct positioning on all screen layouts.
- **Wayland Reliability**: Improved handling of laptop lid events to prevent crashes or layout issues.
- **UI/UX Fixes**:
    - Resolved a bug where workspaces wouldn't update when window titles remained static.
    - Improved error reporting by including the command name in "Command failed" messages.
- **Battery Formatting**: Refined the default formatting and behavior of the battery module.
- **Dependency Management**: Updated core dependencies (including `xcb`) to fix stability issues and address reported bugs.

---

# Oatbar 0.2.0: The AI-Powered Status Bar

We are excited to announce the release of **Oatbar 0.2.0**! This release brings a major new capability to your desktop: first-class integration with Large Language Models (LLMs).

## Major Feature: `oatbar-llm`

Oatbar now ships with `oatbar-llm`, a powerful utility that allows you to pipe system data, logs, or any command output into an LLM and display the processed result on your bar.

- **Multi-Provider Support**: Works out of the box with **Google Gemini**, **OpenAI**, **Anthropic**, **Mistral**, **xAI**, and **Ollama**.
- **Context-Aware**: Feed command outputs (like `git status`, `journalctl`, `weather`) as context to the LLM.
- **Knowledge Base**: Provide static markdown files (style guides, schedules, runbooks) to ground the LLM's responses.
- **Structured Output**: Automatically handles JSON formatting for seamless integration with Oatbar blocks.

Check out the [LLM Cookbook](book/src/configuration/cookbook/llm.md) for examples like:
- **System Health Analyzer**
- **Weather & Outfit Advisor**
- **Hacker News Summarizer**
- **Security Monitor**

## Other Improvements

- **Rotating Logs**: Improved logging infrastructure with file rotation to keep disk usage in check.
- **Default Features**: LLM support is now enabled by default, so you don't need to mess with feature flags to get started.

Enjoy the new release!
