use anyhow::anyhow;

#[allow(unused)]
mod ipc;

fn main() -> anyhow::Result<()> {
    let response = ipc::send_request(ipc::Request::Poke)?;
    if let Some(error) = response.error {
        return Err(anyhow!("{}", error));
    }
    Ok(())
}
