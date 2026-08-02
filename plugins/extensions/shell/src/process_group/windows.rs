use std::{
    io,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
};

use tokio::process::{Child, Command};
use windows_sys::Win32::System::{
    JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject,
    },
    Threading::CREATE_NO_WINDOW,
};

use super::ProcessGroupHolder;

#[derive(Debug)]
pub(crate) struct ProcessGroup {
    job: OwnedHandle,
}

impl ProcessGroupHolder for ProcessGroup {
    fn prepare(cmd: &mut Command) {
        // this stop the console pop up
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    fn from_child(child: &Child) -> io::Result<Self> {
        // create a dummy kernel job and wrap it in OwnedHandle for auto drop/cleanup
        let raw = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if raw.is_null() {
            return Err(io::Error::last_os_error());
        }
        let job = unsafe { OwnedHandle::from_raw_handle(raw) };

        // kill every member process when the last handle to the job closes
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if unsafe {
            SetInformationJobObject(
                job.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                std::ptr::from_ref(&info).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }

        // place the child into the job, it and its children's lifetime will be managed by the job
        let child_handle = child
            .raw_handle()
            .ok_or_else(|| io::Error::other("spawned child has no process handle"))?;

        if unsafe { AssignProcessToJobObject(job.as_raw_handle(), child_handle) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { job })
    }

    fn kill(&self) {
        unsafe { TerminateJobObject(self.job.as_raw_handle(), 1) };
    }
}
