use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crate::command::git;

const CHANNEL_POLL_FALLBACK_MS: u64 = 20;

#[derive(Debug, Clone)]
pub struct CommitEntry {
    pub hash: String,
    pub full_hash: String,
    pub author: String,
    pub date: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct CommitDetail {
    pub full_hash: String,
    pub author: String,
    pub author_email: String,
    pub date: String,
    pub message: String,
    pub files: Vec<CommitFileEntry>,
    pub diff_lines: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct CommitFileEntry {
    pub path: String,
    pub status: char,
}

#[derive(Debug)]
pub enum CommitLogCommand {
    LoadPage {
        project_root: PathBuf,
        skip: usize,
        count: usize,
    },
    LoadDetail {
        project_root: PathBuf,
        hash: String,
    },
    Shutdown,
}

#[derive(Debug)]
pub enum CommitLogEvent {
    PageLoaded {
        commits: Vec<CommitEntry>,
        has_more: bool,
        is_append: bool,
    },
    DetailLoaded {
        hash: String,
        detail: CommitDetail,
    },
    Error {
        message: String,
    },
}

pub struct CommitLogRuntimeHandle {
    pub command_tx: mpsc::Sender<CommitLogCommand>,
    pub event_rx: mpsc::Receiver<CommitLogEvent>,
    _worker_thread: Option<thread::JoinHandle<()>>,
}

impl CommitLogRuntimeHandle {
    pub fn new() -> Result<Self, String> {
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();

        let worker = CommitLogWorker::new(command_rx, event_tx);
        let worker_thread = thread::Builder::new()
            .name("gargo-commit-log-runtime".to_string())
            .spawn(move || worker.run())
            .map_err(|e| format!("failed to spawn commit log runtime worker: {}", e))?;

        Ok(Self {
            command_tx,
            event_rx,
            _worker_thread: Some(worker_thread),
        })
    }
}

impl Drop for CommitLogRuntimeHandle {
    fn drop(&mut self) {
        let _ = self.command_tx.send(CommitLogCommand::Shutdown);
    }
}

struct CommitLogWorker {
    command_rx: mpsc::Receiver<CommitLogCommand>,
    event_tx: mpsc::Sender<CommitLogEvent>,
}

impl CommitLogWorker {
    fn new(
        command_rx: mpsc::Receiver<CommitLogCommand>,
        event_tx: mpsc::Sender<CommitLogEvent>,
    ) -> Self {
        Self {
            command_rx,
            event_tx,
        }
    }

    fn run(self) {
        loop {
            match self
                .command_rx
                .recv_timeout(Duration::from_millis(CHANNEL_POLL_FALLBACK_MS))
            {
                Ok(CommitLogCommand::Shutdown) => break,
                Ok(command) => self.handle_command(command),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    }

    fn handle_command(&self, command: CommitLogCommand) {
        match command {
            CommitLogCommand::LoadPage {
                project_root,
                skip,
                count,
            } => {
                let is_append = skip > 0;
                match load_commits(&project_root, skip, count) {
                    Ok((commits, has_more)) => {
                        let _ = self.event_tx.send(CommitLogEvent::PageLoaded {
                            commits,
                            has_more,
                            is_append,
                        });
                    }
                    Err(message) => {
                        let _ = self.event_tx.send(CommitLogEvent::Error { message });
                    }
                }
            }
            CommitLogCommand::LoadDetail {
                project_root,
                hash,
            } => match load_commit_detail(&project_root, &hash) {
                Ok(detail) => {
                    let _ = self.event_tx.send(CommitLogEvent::DetailLoaded {
                        hash,
                        detail,
                    });
                }
                Err(message) => {
                    let _ = self.event_tx.send(CommitLogEvent::Error { message });
                }
            },
            CommitLogCommand::Shutdown => {}
        }
    }
}

fn load_commits(
    project_root: &std::path::Path,
    skip: usize,
    count: usize,
) -> Result<(Vec<CommitEntry>, bool), String> {
    let raw = git::git_log_oneline_in(project_root, skip, count + 1)?;
    let mut commits = Vec::new();
    for line in raw.lines() {
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.splitn(5, '\x00').collect();
        if parts.len() < 5 {
            continue;
        }
        commits.push(CommitEntry {
            hash: parts[0].to_string(),
            full_hash: parts[1].to_string(),
            author: parts[2].to_string(),
            date: parts[3].to_string(),
            message: parts[4].to_string(),
        });
    }
    let has_more = commits.len() > count;
    commits.truncate(count);
    Ok((commits, has_more))
}

fn load_commit_detail(
    project_root: &std::path::Path,
    hash: &str,
) -> Result<CommitDetail, String> {
    // Get metadata + full message
    let meta_raw = git::git_show_metadata_in(project_root, hash)?;
    let meta_lines: Vec<&str> = meta_raw.splitn(5, '\n').collect();
    let full_hash = meta_lines.first().unwrap_or(&"").to_string();
    let author = meta_lines.get(1).unwrap_or(&"").to_string();
    let author_email = meta_lines.get(2).unwrap_or(&"").to_string();
    let date = meta_lines.get(3).unwrap_or(&"").to_string();
    let message = meta_lines.get(4).unwrap_or(&"").to_string();

    // Get changed files
    let files_raw = git::git_diff_tree_in(project_root, hash)?;
    let mut files = Vec::new();
    for line in files_raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.splitn(2, '\t');
        let status = parts
            .next()
            .and_then(|s| s.chars().next())
            .unwrap_or('M');
        let path = parts.next().unwrap_or("").to_string();
        if !path.is_empty() {
            files.push(CommitFileEntry { path, status });
        }
    }

    // Get diff
    let diff_raw = git::git_show_diff_in(project_root, hash)?;
    let diff_lines = diff_raw.lines().map(|l| l.to_string()).collect();

    Ok(CommitDetail {
        full_hash,
        author,
        author_email,
        date,
        message,
        files,
        diff_lines,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
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
    fn loads_commits_from_repo() {
        let temp = tempfile::tempdir().expect("create tempdir");
        run_git(temp.path(), &["init"]);
        run_git(temp.path(), &["config", "user.name", "gargo-test"]);
        run_git(
            temp.path(),
            &["config", "user.email", "gargo-test@example.com"],
        );
        fs::write(temp.path().join("a.txt"), "hello\n").expect("write");
        run_git(temp.path(), &["add", "a.txt"]);
        run_git(temp.path(), &["commit", "-m", "initial commit"]);

        let (commits, has_more) = load_commits(temp.path(), 0, 100).expect("load commits");
        assert_eq!(commits.len(), 1);
        assert!(!has_more);
        assert_eq!(commits[0].message, "initial commit");
    }

    #[test]
    fn runtime_loads_commits() {
        let temp = tempfile::tempdir().expect("create tempdir");
        run_git(temp.path(), &["init"]);
        run_git(temp.path(), &["config", "user.name", "gargo-test"]);
        run_git(
            temp.path(),
            &["config", "user.email", "gargo-test@example.com"],
        );
        fs::write(temp.path().join("a.txt"), "hello\n").expect("write");
        run_git(temp.path(), &["add", "a.txt"]);
        run_git(temp.path(), &["commit", "-m", "test commit"]);

        let runtime = CommitLogRuntimeHandle::new().expect("start runtime");
        runtime
            .command_tx
            .send(CommitLogCommand::LoadPage {
                project_root: temp.path().to_path_buf(),
                skip: 0,
                count: 100,
            })
            .expect("send command");

        let event = runtime
            .event_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("recv event");
        match event {
            CommitLogEvent::PageLoaded { commits, .. } => {
                assert_eq!(commits.len(), 1);
                assert_eq!(commits[0].message, "test commit");
            }
            other => panic!("unexpected event: {:?}", other),
        }
    }
}
