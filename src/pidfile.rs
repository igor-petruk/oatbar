use std::io::Write;

pub struct Pidfile {
    path: std::path::PathBuf,
}

impl Pidfile {
    pub fn new() -> anyhow::Result<Self> {
        let mut path = dirs::runtime_dir()
            .or_else(dirs::state_dir)
            .unwrap_or_else(std::env::temp_dir);
        path.push("oatbar/oatbar.pid");
        Ok(Self { path })
    }

    pub fn write(&self) -> anyhow::Result<()> {
        let pid = std::process::id();
        std::fs::create_dir_all(
            self.path
                .parent()
                .ok_or_else(|| anyhow::anyhow!("Cannot get parent dir of {:?}", self.path))?,
        )?;
        let mut file = std::fs::File::create(&self.path)?;
        file.write_all(format!("{}", pid).as_bytes())?;
        Ok(())
    }

    pub fn read(&self) -> anyhow::Result<u32> {
        let pid_str = std::fs::read_to_string(&self.path)?;
        Ok(pid_str.parse()?)
    }
}
