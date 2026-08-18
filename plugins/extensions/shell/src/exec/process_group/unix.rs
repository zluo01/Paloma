use std::io;

use nix::{
    sys::signal::{Signal, killpg},
    unistd::Pid,
};
use tokio::process::{Child, Command};

use super::ProcessGroupHolder;

#[derive(Debug)]
pub(crate) struct ProcessGroup {
    process_group_id: i32,
}

impl ProcessGroupHolder for ProcessGroup {
    fn prepare(cmd: &mut Command) {
        // make this command process its own process-group leader
        cmd.process_group(0);
    }

    fn from_child(child: &Child) -> io::Result<Self> {
        let pid = child
            .id()
            .ok_or_else(|| io::Error::other("spawned child has no pid"))?;
        Ok(Self {
            process_group_id: pid as i32,
        })
    }

    fn kill(&self) {
        let _ = killpg(Pid::from_raw(self.process_group_id), Signal::SIGKILL);
    }
}
