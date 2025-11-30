use anyhow::Context;
use std::collections::HashMap;
use std::process::Command;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct Notifier {
    ids: Arc<Mutex<HashMap<String, u32>>>,
}

impl Notifier {
    pub fn new() -> Self {
        Self {
            ids: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn is_installed() -> bool {
        Command::new("which")
            .arg("notify-send")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    pub fn send(&self, name: &str, summary: &str, body: &str) -> anyhow::Result<bool> {
        if !Self::is_installed() {
            return Ok(false);
        }

        let mut ids = self.ids.lock().unwrap();

        let mut command = Command::new("notify-send");
        command.arg("-p"); // Print the notification ID

        if let Some(id) = ids.get(name) {
            command.arg("-r").arg(id.to_string());
        }

        command.arg(summary);
        command.arg(body);

        let output = command.output().context("Failed to execute notify-send")?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "notify-send failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        let output_str = String::from_utf8_lossy(&output.stdout);
        if let Ok(id) = output_str.trim().parse::<u32>() {
            ids.insert(name.to_string(), id);
        }

        Ok(true)
    }
}
