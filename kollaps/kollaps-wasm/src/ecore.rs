use crate::{config::Config, service::ActiveService};

use std::{
    fs::File,
    process::{Child, Command},
};

use anyhow::Result;
use tracing::debug;

/// Represents a running Emulationcore instance.
/// Dropping this struct will kill the respective process.
pub struct EmulationCore {
    // For debugging only. Represents which service this instance is created for.
    service_id: String,
    // Handle to this emulationcore process.
    child: Child,
}

impl EmulationCore {
    /// Try to spawn an emulationcore process, returning self if successful.
    pub fn try_new(config: &Config, service: &ActiveService) -> Result<Self> {
        let logs_dir = &config
            .logs_dir
            .join(format!(".emulationcore_{}.txt", service.id()));
        let stdout = File::create(logs_dir)?;
        let child = Command::new("ip")
            .args(["netns", "exec", service.ns_name()])
            .args([
                &config
                    .executables_dir
                    .join("emulationcore")
                    .to_string_lossy(),
                &config.topology_path.canonicalize()?.to_string_lossy(),
                "-i",
                service.veth(),
                "wasm",
                service.id(),
                &service.pid().to_string(),
            ])
            .stdout(stdout.try_clone()?)
            .stderr(stdout)
            .current_dir(config.executables_dir.parent().unwrap())
            .spawn()?;

        let service_id = service.id().to_owned();

        Ok(Self { service_id, child })
    }

    /// Checks if the process belonging to this object have exited.
    /// If it has, it returns `Some(exit_status)`, otherwise `None`.
    /// An `exit_status` of true means that the process has returned 0 as return value.
    pub fn try_wait(&mut self) -> Option<bool> {
        if let Ok(res) = self.child.try_wait()
            && let Some(exit_status) = res
        {
            return Some(exit_status.success());
        }
        None
    }
}

impl Drop for EmulationCore {
    fn drop(&mut self) {
        let res = self.child.kill();
        debug!(
            "Sent SIGINT to emulationcore instance ({}) of {}, result was {:?}",
            self.child.id(),
            self.service_id,
            res
        );
    }
}
