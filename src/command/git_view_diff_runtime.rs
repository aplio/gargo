use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crate::command::git;

const CHANNEL_POLL_FALLBACK_MS: u64 = 20;
const PREFETCH_DEBOUNCE_MS: u64 = 24;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DiffCacheKey {
    pub path: String,
    pub staged: bool,
}

#[derive(Debug)]
pub enum GitViewDiffCommand {
    RequestDiff {
        request_id: u64,
        project_root: PathBuf,
        key: DiffCacheKey,
        high_priority: bool,
    },
    Shutdown,
}

#[derive(Debug)]
pub enum GitViewDiffEvent {
    DiffReady {
        request_id: u64,
        key: DiffCacheKey,
        lines: Vec<String>,
    },
    DiffError {
        request_id: u64,
        key: DiffCacheKey,
        message: String,
    },
}

pub struct GitViewDiffRuntimeHandle {
    pub command_tx: mpsc::Sender<GitViewDiffCommand>,
    pub event_rx: mpsc::Receiver<GitViewDiffEvent>,
    _worker_thread: Option<thread::JoinHandle<()>>,
}

impl GitViewDiffRuntimeHandle {
    pub fn new() -> Result<Self, String> {
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();

        let worker = GitViewDiffWorker::new(command_rx, event_tx);
        let worker_thread = thread::Builder::new()
            .name("gargo-git-view-diff-runtime".to_string())
            .spawn(move || worker.run())
            .map_err(|e| format!("failed to spawn git view diff runtime worker: {}", e))?;

        Ok(Self {
            command_tx,
            event_rx,
            _worker_thread: Some(worker_thread),
        })
    }
}

impl Drop for GitViewDiffRuntimeHandle {
    fn drop(&mut self) {
        let _ = self.command_tx.send(GitViewDiffCommand::Shutdown);
    }
}

struct PendingDiffRequest {
    request_id: u64,
    project_root: PathBuf,
    key: DiffCacheKey,
    due: Instant,
}

struct GitViewDiffWorker {
    command_rx: mpsc::Receiver<GitViewDiffCommand>,
    event_tx: mpsc::Sender<GitViewDiffEvent>,
    pending: HashMap<DiffCacheKey, PendingDiffRequest>,
}

impl GitViewDiffWorker {
    fn new(
        command_rx: mpsc::Receiver<GitViewDiffCommand>,
        event_tx: mpsc::Sender<GitViewDiffEvent>,
    ) -> Self {
        Self {
            command_rx,
            event_tx,
            pending: HashMap::new(),
        }
    }

    fn run(mut self) {
        loop {
            let timeout = self.next_timeout();
            let received = self.command_rx.recv_timeout(timeout);
            match received {
                Ok(GitViewDiffCommand::Shutdown) => break,
                Ok(command) => self.handle_command(command),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }

            while let Ok(command) = self.command_rx.try_recv() {
                if matches!(command, GitViewDiffCommand::Shutdown) {
                    return;
                }
                self.handle_command(command);
            }

            self.process_due_requests();
        }
    }

    fn handle_command(&mut self, command: GitViewDiffCommand) {
        match command {
            GitViewDiffCommand::RequestDiff {
                request_id,
                project_root,
                key,
                high_priority,
            } => {
                let due = if high_priority {
                    Instant::now()
                } else {
                    Instant::now() + Duration::from_millis(PREFETCH_DEBOUNCE_MS)
                };
                if let Some(pending) = self.pending.get_mut(&key) {
                    pending.request_id = request_id;
                    pending.project_root = project_root;
                    pending.due = pending.due.min(due);
                } else {
                    self.pending.insert(
                        key.clone(),
                        PendingDiffRequest {
                            request_id,
                            project_root,
                            key,
                            due,
                        },
                    );
                }
            }
            GitViewDiffCommand::Shutdown => {}
        }
    }

    fn process_due_requests(&mut self) {
        let now = Instant::now();
        let ready_keys: Vec<DiffCacheKey> = self
            .pending
            .iter()
            .filter_map(|(key, pending)| (pending.due <= now).then_some(key.clone()))
            .collect();

        for key in ready_keys {
            let Some(pending) = self.pending.remove(&key) else {
                continue;
            };

            match git::git_diff_in(&pending.project_root, &pending.key.path, pending.key.staged) {
                Ok(diff) => {
                    let lines = diff.lines().map(|line| line.to_string()).collect();
                    let _ = self.event_tx.send(GitViewDiffEvent::DiffReady {
                        request_id: pending.request_id,
                        key: pending.key,
                        lines,
                    });
                }
                Err(message) => {
                    let _ = self.event_tx.send(GitViewDiffEvent::DiffError {
                        request_id: pending.request_id,
                        key: pending.key,
                        message,
                    });
                }
            }
        }
    }

    fn next_timeout(&self) -> Duration {
        let now = Instant::now();
        let next_due = self.pending.values().map(|pending| pending.due).min();
        match next_due {
            Some(due) if due > now => due.duration_since(now),
            Some(_) => Duration::from_millis(0),
            None => Duration::from_millis(CHANNEL_POLL_FALLBACK_MS),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn run_git(repo: &std::path::Path, args: &[&str]) {
        let status = Command::new("git")
            .current_dir(repo)
            .args(args)
            .status()
            .expect("run git command");
        assert!(status.success(), "git {:?} failed", args);
    }

    #[test]
    fn coalesces_duplicate_requests_for_same_key() {
        let temp = tempfile::tempdir().expect("create tempdir");
        run_git(temp.path(), &["init"]);
        run_git(temp.path(), &["config", "user.name", "gargo-test"]);
        run_git(
            temp.path(),
            &["config", "user.email", "gargo-test@example.com"],
        );
        std::fs::write(temp.path().join("sample.txt"), "line1\n").expect("write sample");
        run_git(temp.path(), &["add", "sample.txt"]);
        run_git(temp.path(), &["commit", "-m", "init"]);
        std::fs::write(temp.path().join("sample.txt"), "line1\nline2\n").expect("modify sample");

        let runtime = GitViewDiffRuntimeHandle::new().expect("start runtime");
        let key = DiffCacheKey {
            path: "sample.txt".to_string(),
            staged: false,
        };

        runtime
            .command_tx
            .send(GitViewDiffCommand::RequestDiff {
                request_id: 1,
                project_root: temp.path().to_path_buf(),
                key: key.clone(),
                high_priority: true,
            })
            .expect("queue first diff request");
        runtime
            .command_tx
            .send(GitViewDiffCommand::RequestDiff {
                request_id: 2,
                project_root: temp.path().to_path_buf(),
                key,
                high_priority: true,
            })
            .expect("queue second diff request");

        let event = runtime
            .event_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("recv diff event");
        match event {
            GitViewDiffEvent::DiffReady {
                request_id, lines, ..
            } => {
                assert_eq!(request_id, 2);
                assert!(!lines.is_empty());
            }
            other => panic!("unexpected event: {:?}", other),
        }

        let none = runtime.event_rx.recv_timeout(Duration::from_millis(100));
        assert!(matches!(none, Err(mpsc::RecvTimeoutError::Timeout)));
    }

    #[test]
    fn emits_error_for_non_repo_request() {
        let temp = tempfile::tempdir().expect("create tempdir");
        let runtime = GitViewDiffRuntimeHandle::new().expect("start runtime");
        runtime
            .command_tx
            .send(GitViewDiffCommand::RequestDiff {
                request_id: 9,
                project_root: temp.path().to_path_buf(),
                key: DiffCacheKey {
                    path: "missing.txt".to_string(),
                    staged: false,
                },
                high_priority: true,
            })
            .expect("queue diff request");

        let event = runtime
            .event_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("recv event");
        match event {
            GitViewDiffEvent::DiffError { request_id, .. } => {
                assert_eq!(request_id, 9);
            }
            other => panic!("unexpected event: {:?}", other),
        }
    }
}
