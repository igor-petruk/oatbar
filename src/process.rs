pub fn run_detached(command: &str, envs: Vec<(String, String)>) -> anyhow::Result<()> {
    match fork::fork() {
        Err(e) => {
            tracing::error!("Failed to spawn {:?}: {:?}", command, e);
        }
        Ok(fork::Fork::Parent(mut pid)) => unsafe {
            libc::wait(&mut pid);
        },
        Ok(fork::Fork::Child) => {
            if let Err(e) = fork::setsid() {
                tracing::error!("Failed to setsid() {:?}: {:?}", command, e);
                std::process::exit(1);
            }
            match fork::fork() {
                Err(e) => {
                    tracing::error!("Failed to spawn {:?}: {:?}", command, e);
                    std::process::exit(1);
                }
                Ok(fork::Fork::Parent(_)) => {
                    std::process::exit(0);
                }
                Ok(fork::Fork::Child) => {
                    use std::os::unix::process::CommandExt;
                    let _ = std::process::Command::new("sh")
                        .arg("-c")
                        .arg(command)
                        .envs(envs)
                        .exec();
                }
            }
        }
    }
    tracing::info!("{:?} spawned", command);
    Ok(())
}
