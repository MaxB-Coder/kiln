//! What runs under a shell, and the ports it listens on, for the context rail.

use std::collections::{HashMap, HashSet};
use std::net::IpAddr;

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

#[derive(Clone, Debug, PartialEq)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    /// The full command line, space-joined.
    pub command: String,
    /// Percent of one CPU core (can exceed 100 for multithreaded programs).
    pub cpu: f32,
    pub memory_bytes: u64,
    /// 0 for the shell's direct children.
    pub depth: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortInfo {
    pub port: u16,
    pub address: IpAddr,
    pub pid: u32,
    pub process: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProcessReport {
    pub processes: Vec<ProcessInfo>,
    pub ports: Vec<PortInfo>,
}

/// Samples the process table. Keep one and call [`refresh`](Self::refresh) every second or two:
/// CPU usage is measured between refreshes.
pub struct ProcessWatcher {
    system: System,
    listening: Vec<listeners::Listener>,
}

impl Default for ProcessWatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessWatcher {
    pub fn new() -> Self {
        Self {
            system: System::new(),
            listening: Vec::new(),
        }
    }

    pub fn refresh(&mut self) {
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing()
                .with_cpu()
                .with_memory()
                .with_cmd(UpdateKind::OnlyIfNotSet),
        );
        self.listening = listeners::get_all()
            .map(|all| {
                all.into_iter()
                    .filter(|l| {
                        l.protocol == listeners::Protocol::TCP
                            && l.state == listeners::SocketState::Listen
                    })
                    .collect()
            })
            .unwrap_or_default();
    }

    /// Everything running under `root` (not `root` itself), parents before children.
    pub fn report(&self, root: u32) -> ProcessReport {
        let mut children: HashMap<Pid, Vec<Pid>> = HashMap::new();
        for (pid, process) in self.system.processes() {
            if let Some(parent) = process.parent() {
                children.entry(parent).or_default().push(*pid);
            }
        }
        for list in children.values_mut() {
            list.sort();
        }

        let mut processes = Vec::new();
        let mut stack: Vec<(Pid, usize)> = children
            .get(&Pid::from_u32(root))
            .into_iter()
            .flatten()
            .rev()
            .map(|p| (*p, 0))
            .collect();
        let mut seen = HashSet::new();
        while let Some((pid, depth)) = stack.pop() {
            if !seen.insert(pid) {
                continue;
            }
            let Some(process) = self.system.process(pid) else {
                continue;
            };
            // Threads show up as tasks on Linux; only list real processes.
            if process.thread_kind().is_some() {
                continue;
            }
            let command = process
                .cmd()
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join(" ");
            processes.push(ProcessInfo {
                pid: pid.as_u32(),
                name: process.name().to_string_lossy().into_owned(),
                command,
                cpu: process.cpu_usage(),
                memory_bytes: process.memory(),
                depth,
            });
            if let Some(kids) = children.get(&pid) {
                stack.extend(kids.iter().rev().map(|p| (*p, depth + 1)));
            }
        }

        let pids: HashSet<u32> = processes.iter().map(|p| p.pid).collect();
        let mut ports: Vec<PortInfo> = self
            .listening
            .iter()
            .filter(|l| pids.contains(&l.process.pid))
            .map(|l| PortInfo {
                port: l.socket.port(),
                address: l.socket.ip(),
                pid: l.process.pid,
                process: l.process.name.clone(),
            })
            .collect();
        ports.sort_by_key(|p| (p.port, p.address.is_ipv6()));
        // One row per port: IPv4 and IPv6 listeners on the same port are the same thing here.
        ports.dedup_by_key(|p| p.port);

        ProcessReport { processes, ports }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(unix)]
    fn finds_a_child_process_and_its_port() {
        use std::net::TcpListener;
        // A child of this test process that listens on a port: here, the test itself listens and
        // a `sleep` child stands in for a running command.
        let mut child = std::process::Command::new("sleep")
            .arg("5")
            .spawn()
            .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let mut watcher = ProcessWatcher::new();
        watcher.refresh();
        let report = watcher.report(std::process::id());
        assert!(
            report
                .processes
                .iter()
                .any(|p| p.pid == child.id() && p.name.contains("sleep"))
        );
        // The test process is the root, so its own port is not "under" it.
        assert!(report.ports.iter().all(|p| p.port != port));
        child.kill().unwrap();
        let _ = child.wait();
    }
}
