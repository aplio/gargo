use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

const CHANNEL_POLL_FALLBACK_MS: u64 = 20;

#[derive(Debug)]
pub enum FileIndexRuntimeCommand {
    Refresh { project_root: PathBuf },
    Shutdown,
}

#[derive(Debug)]
pub enum FileIndexRuntimeEvent {
    Ready {
        project_root: PathBuf,
        files: Vec<String>,
    },
}

pub struct FileIndexRuntimeHandle {
    pub command_tx: mpsc::Sender<FileIndexRuntimeCommand>,
    pub event_rx: mpsc::Receiver<FileIndexRuntimeEvent>,
    _worker_thread: Option<thread::JoinHandle<()>>,
}

impl FileIndexRuntimeHandle {
    pub fn new() -> Result<Self, String> {
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();

        let worker = FileIndexRuntimeWorker::new(command_rx, event_tx);
        let worker_thread = thread::Builder::new()
            .name("gargo-file-index-runtime".to_string())
            .spawn(move || worker.run())
            .map_err(|e| format!("failed to spawn file index runtime worker: {}", e))?;

        Ok(Self {
            command_tx,
            event_rx,
            _worker_thread: Some(worker_thread),
        })
    }
}

impl Drop for FileIndexRuntimeHandle {
    fn drop(&mut self) {
        let _ = self.command_tx.send(FileIndexRuntimeCommand::Shutdown);
    }
}

struct FileIndexRuntimeWorker {
    command_rx: mpsc::Receiver<FileIndexRuntimeCommand>,
    event_tx: mpsc::Sender<FileIndexRuntimeEvent>,
    pending_project_root: Option<PathBuf>,
}

impl FileIndexRuntimeWorker {
    fn new(
        command_rx: mpsc::Receiver<FileIndexRuntimeCommand>,
        event_tx: mpsc::Sender<FileIndexRuntimeEvent>,
    ) -> Self {
        Self {
            command_rx,
            event_tx,
            pending_project_root: None,
        }
    }

    fn run(mut self) {
        loop {
            match self
                .command_rx
                .recv_timeout(Duration::from_millis(CHANNEL_POLL_FALLBACK_MS))
            {
                Ok(FileIndexRuntimeCommand::Refresh { project_root }) => {
                    self.pending_project_root = Some(project_root);
                }
                Ok(FileIndexRuntimeCommand::Shutdown) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }

            if let Some(project_root) = self.pending_project_root.take() {
                let files = crate::io::file_io::collect_files(&project_root);
                let _ = self.event_tx.send(FileIndexRuntimeEvent::Ready {
                    project_root,
                    files,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn refresh_returns_file_list() {
        let tmp = tempdir().expect("create temp dir");
        fs::create_dir_all(tmp.path().join(".git")).expect("create git dir");
        fs::write(tmp.path().join("a.txt"), "a").expect("write a");
        fs::write(tmp.path().join("b.txt"), "b").expect("write b");

        let runtime = FileIndexRuntimeHandle::new().expect("create runtime");
        runtime
            .command_tx
            .send(FileIndexRuntimeCommand::Refresh {
                project_root: tmp.path().to_path_buf(),
            })
            .expect("send refresh");

        let event = runtime
            .event_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("receive indexed files");
        match event {
            FileIndexRuntimeEvent::Ready {
                project_root,
                files,
            } => {
                assert_eq!(project_root, tmp.path());
                assert!(files.contains(&"a.txt".to_string()));
                assert!(files.contains(&"b.txt".to_string()));
            }
        }
    }
}
