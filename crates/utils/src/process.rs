use std::{future::Future, time::Duration};

use command_group::AsyncGroupChild;

const PROCESS_KILL_TIMEOUT: Duration = Duration::from_secs(5);
const PROCESS_EXIT_WAIT_TIMEOUT: Duration = Duration::from_secs(5);

/// Pin the provider's existing descendants before native interrupt can orphan
/// them. Linux tool jobs can create separate sessions, outside the provider PG.
/// pidfds prevent a recycled PID from targeting an unrelated process. This is a
/// cancellation snapshot, not a sandbox or a guarantee against daemonisation.
#[derive(Default)]
pub struct CancelledDescendants {
    #[cfg(target_os = "linux")]
    processes: Vec<std::os::fd::OwnedFd>,
}

impl CancelledDescendants {
    pub fn capture(root: Option<u32>) -> std::io::Result<Self> {
        #[cfg(target_os = "linux")]
        {
            use std::{
                collections::{HashMap, HashSet},
                os::fd::FromRawFd,
            };
            let Some(root) = root else {
                return Ok(Self::default());
            };
            let mut entries = HashMap::new();
            for entry in std::fs::read_dir("/proc")? {
                let entry = entry?;
                let Some(pid) = entry
                    .file_name()
                    .to_str()
                    .and_then(|s| s.parse::<u32>().ok())
                else {
                    continue;
                };
                if let Ok(identity) = linux_process_identity(pid) {
                    entries.insert(pid, identity);
                }
            }
            let mut descendants = HashSet::from([root]);
            loop {
                let before = descendants.len();
                for (&pid, &(parent, _)) in &entries {
                    if descendants.contains(&parent) {
                        descendants.insert(pid);
                    }
                }
                if before == descendants.len() {
                    break;
                }
            }
            descendants.remove(&root);
            let mut processes = Vec::new();
            for pid in descendants {
                // SAFETY: pidfd_open takes integer arguments and returns an
                // owned descriptor; it does not signal or dereference memory.
                let fd = unsafe { nix::libc::syscall(nix::libc::SYS_pidfd_open, pid, 0) };
                if fd < 0 {
                    let error = std::io::Error::last_os_error();
                    if error.raw_os_error() == Some(nix::libc::ESRCH) {
                        continue;
                    }
                    return Err(error);
                }
                // SAFETY: the successfully opened descriptor is owned here.
                let fd = unsafe { std::os::fd::OwnedFd::from_raw_fd(fd as i32) };
                // A PID may have exited/recycled between enumeration and open.
                // Only retain the descriptor when its original identity agrees.
                if linux_process_identity(pid).ok().as_ref() == entries.get(&pid) {
                    processes.push(fd);
                }
            }
            Ok(Self { processes })
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = root;
            Ok(Self::default())
        }
    }

    pub async fn terminate(self) -> std::io::Result<()> {
        #[cfg(target_os = "linux")]
        {
            use std::os::fd::AsRawFd;
            for fd in &self.processes {
                // SAFETY: pidfd is live; null siginfo requests a normal signal.
                let result = unsafe {
                    nix::libc::syscall(
                        nix::libc::SYS_pidfd_send_signal,
                        fd.as_raw_fd(),
                        nix::libc::SIGKILL,
                        std::ptr::null::<nix::libc::siginfo_t>(),
                        0,
                    )
                };
                if result < 0 {
                    let error = std::io::Error::last_os_error();
                    if error.raw_os_error() != Some(nix::libc::ESRCH) {
                        return Err(error);
                    }
                }
            }
            tokio::time::timeout(PROCESS_EXIT_WAIT_TIMEOUT, async {
                loop {
                    let mut all_exited = true;
                    for fd in &self.processes {
                        let mut poll = nix::libc::pollfd {
                            fd: fd.as_raw_fd(),
                            events: nix::libc::POLLIN,
                            revents: 0,
                        };
                        // SAFETY: valid one-element pollfd buffer, nonblocking.
                        if unsafe { nix::libc::poll(&mut poll, 1, 0) } < 0 {
                            return Err(std::io::Error::last_os_error());
                        }
                        all_exited &= poll.revents & nix::libc::POLLIN != 0;
                    }
                    if all_exited {
                        return Ok(());
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "cancelled provider descendants did not exit",
                )
            })??;
        }
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn linux_process_identity(pid: u32) -> std::io::Result<(u32, u64)> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))?;
    // comm may contain spaces or ')'; fields after its final ')' start at state.
    let fields: Vec<_> = stat
        .rsplit_once(')')
        .map(|(_, tail)| tail.split_whitespace().collect())
        .unwrap_or_default();
    let parent = fields.get(1).and_then(|s| s.parse().ok());
    let start = fields.get(19).and_then(|s| s.parse().ok());
    parent.zip(start).ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid process identity")
    })
}

async fn io_with_timeout<T>(
    future: impl Future<Output = std::io::Result<T>>,
    timeout: Duration,
    operation: &'static str,
) -> std::io::Result<T> {
    tokio::time::timeout(timeout, future).await.map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            format!("timed out while {operation}"),
        )
    })?
}

pub async fn kill_process_group(child: &mut AsyncGroupChild) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        // Use command_group's UnixChildExt::signal() which calls killpg()
        // with the pgid captured at spawn time. This works even after the
        // group leader has exited, unlike getpgid() which would fail.
        use command_group::{Signal, UnixChildExt};

        for sig in [Signal::SIGINT, Signal::SIGTERM, Signal::SIGKILL] {
            tracing::info!("Sending {:?} to process group", sig);
            if let Err(e) = child.signal(sig) {
                // break if the group does not exist anymore
                if e.raw_os_error() == Some(nix::libc::ESRCH) {
                    break;
                }
                tracing::warn!("Failed to send signal {:?} to process group: {}", sig, e);
            }
            if sig != Signal::SIGKILL {
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }

    let kill_result = io_with_timeout(
        child.kill(),
        PROCESS_KILL_TIMEOUT,
        "terminating process group",
    )
    .await;
    match io_with_timeout(
        child.wait(),
        PROCESS_EXIT_WAIT_TIMEOUT,
        "waiting for process group exit",
    )
    .await
    {
        // A completed wait is the authoritative proof that the process is gone.
        Ok(_) => Ok(()),
        Err(wait_error) => match kill_result {
            Ok(()) => Err(wait_error),
            Err(kill_error) => Err(std::io::Error::new(
                wait_error.kind(),
                format!("{kill_error}; {wait_error}"),
            )),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn cancellation_cleans_up_orphaned_session_children_not_other_processes() {
        use command_group::AsyncCommandGroup;
        use tokio::io::{AsyncBufReadExt, BufReader};
        let mut unrelated = tokio::process::Command::new("sleep")
            .arg("120")
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut command = tokio::process::Command::new("sh");
        command
            .args(["-c", "setsid sh -c 'sleep 120 & echo $!; wait' & wait"])
            .stdout(std::process::Stdio::piped());
        let mut provider = command.group_spawn().unwrap();
        let mut line = String::new();
        BufReader::new(provider.inner().stdout.take().unwrap())
            .read_line(&mut line)
            .await
            .unwrap();
        let sleeper: u32 = line.trim().parse().unwrap();
        let captured = CancelledDescendants::capture(provider.inner().id()).unwrap();
        assert!(captured.processes.len() >= 2);
        // Native interruption/host exit may leave the separate session alive.
        provider.kill().await.unwrap();
        captured.terminate().await.unwrap();
        let stat = std::fs::read_to_string(format!("/proc/{sleeper}/stat"));
        assert!(
            stat.is_err()
                || stat
                    .unwrap()
                    .rsplit_once(')')
                    .unwrap()
                    .1
                    .trim_start()
                    .starts_with('Z')
        );
        assert!(unrelated.try_wait().unwrap().is_none());
        unrelated.kill().await.unwrap();
    }

    #[tokio::test]
    async fn bounded_io_returns_timed_out_instead_of_waiting_forever() {
        let error = io_with_timeout(
            std::future::pending::<std::io::Result<()>>(),
            Duration::from_millis(10),
            "testing bounded wait",
        )
        .await
        .expect_err("pending I/O must time out");

        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        assert!(error.to_string().contains("testing bounded wait"));
    }
}
