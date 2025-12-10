use std::{
    fs::File,
    process::{Child, Command},
};

use anyhow::{Context, Result};
use tracing::debug;

use crate::config::Config;

pub struct CommunicationManager {
    child: Child,
}

impl CommunicationManager {
    pub fn try_new(config: &Config, service_count: usize) -> Result<Self> {
        let stdout = File::create(config.logs_dir.join(".communicationmanager.txt"))?;

        let child = Command::new(config.executables_dir.join("communicationmanager"))
            .arg(service_count.to_string())
            .stdout(stdout.try_clone()?)
            .stderr(stdout)
            .spawn()
            .context("failed to start communicationmanager")?;

        Ok(Self { child })
    }
}

impl Drop for CommunicationManager {
    fn drop(&mut self) {
        let res = self.child.kill();
        debug!(
            "Sent SIGINT to communicationmanager ({}), result was {:?}",
            self.child.id(),
            res
        );
    }
}
