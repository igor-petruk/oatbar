#[allow(unused)]
mod pidfile;

fn main() -> anyhow::Result<()> {
    let pid = pidfile::Pidfile::new()?.read()?;
    let mut child = std::process::Command::new("kill")
        .arg("-USR1")
        .arg(format!("{}", pid))
        .spawn()?;
    let _ = child.wait();
    Ok(())
}
