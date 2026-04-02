use crate::{ipc, process};
use anyhow::anyhow;

pub fn restart_oatbar(instance_name: &str) -> anyhow::Result<()> {
    let client = ipc::Client::new(instance_name)?;
    let process_response = client.send_command(ipc::Command::GetProcessInfo {})?;

    if let Some(error) = process_response.error {
        return Err(anyhow!("{}", error));
    }

    if let Some(ipc::ResponseData::ProcessInfo {
        pid: _,
        command_line,
    }) = process_response.data
    {
        let _ = client.send_command(ipc::Command::Terminate {});

        let path = ipc::socket_path(instance_name)?;
        let mut tries = 0;
        while std::os::unix::net::UnixStream::connect(&path).is_ok() {
            if tries >= 10 {
                return Err(anyhow::anyhow!(
                    "Failed to terminate oatbar securely within 1 second"
                ));
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
            tries += 1;
        }

        let cmd = command_line.join(" ");
        process::run_detached(&cmd, vec![], true)?;
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "Unexpected response format for GetProcessInfo"
        ))
    }
}
