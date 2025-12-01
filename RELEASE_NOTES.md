# Oatbar 0.2.0: The AI-Powered Status Bar

We are excited to announce the release of **Oatbar 0.2.0**! This release brings a major new capability to your desktop: first-class integration with Large Language Models (LLMs).

## :rocket: Major Feature: `oatbar-llm`

Oatbar now ships with `oatbar-llm`, a powerful utility that allows you to pipe system data, logs, or any command output into an LLM and display the processed result on your bar.

- **Multi-Provider Support**: Works out of the box with **Google Gemini**, **OpenAI**, **Anthropic**, **Mistral**, **xAI**, and **Ollama**.
- **Context-Aware**: Feed command outputs (like `git status`, `journalctl`, `weather`) as context to the LLM.
- **Knowledge Base**: Provide static markdown files (style guides, schedules, runbooks) to ground the LLM's responses.
- **Structured Output**: Automatically handles JSON formatting for seamless integration with Oatbar blocks.

Check out the [LLM Cookbook](book/src/configuration/cookbook/llm.md) for examples like:
- :hospital: **System Health Analyzer**
- :partly_sunny: **Weather & Outfit Advisor**
- :newspaper: **Hacker News Summarizer**
- :shield: **Security Monitor**

## :sparkles: Other Improvements

- **Rotating Logs**: Improved logging infrastructure with file rotation to keep disk usage in check.
- **Default Features**: LLM support is now enabled by default, so you don't need to mess with feature flags to get started.

Enjoy the new release!
