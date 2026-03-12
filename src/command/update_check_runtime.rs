use std::sync::mpsc;
use std::thread;

use crate::upgrade::UpgradeCheckStatus;

#[derive(Debug)]
pub enum UpdateCheckRuntimeEvent {
    Ready(Result<UpgradeCheckStatus, String>),
}

pub struct UpdateCheckRuntimeHandle {
    pub event_rx: mpsc::Receiver<UpdateCheckRuntimeEvent>,
    _worker_thread: Option<thread::JoinHandle<()>>,
}

impl UpdateCheckRuntimeHandle {
    pub fn new() -> Result<Self, String> {
        Self::new_with_checker(crate::upgrade::check_status)
    }

    fn new_with_checker<F>(checker: F) -> Result<Self, String>
    where
        F: FnOnce() -> Result<UpgradeCheckStatus, String> + Send + 'static,
    {
        let (event_tx, event_rx) = mpsc::channel();
        let worker_thread = thread::Builder::new()
            .name("gargo-update-check-runtime".to_string())
            .spawn(move || {
                let _ = event_tx.send(UpdateCheckRuntimeEvent::Ready(checker()));
            })
            .map_err(|e| format!("failed to spawn update check runtime worker: {}", e))?;

        Ok(Self {
            event_rx,
            _worker_thread: Some(worker_thread),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn runtime_emits_update_status() {
        let runtime = UpdateCheckRuntimeHandle::new_with_checker(|| {
            Ok(UpgradeCheckStatus::UpdateAvailable {
                current: "0.1.19".to_string(),
                latest: "0.1.20".to_string(),
            })
        })
        .expect("start runtime");

        let event = runtime
            .event_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("receive event");

        match event {
            UpdateCheckRuntimeEvent::Ready(Ok(UpgradeCheckStatus::UpdateAvailable {
                current,
                latest,
            })) => {
                assert_eq!(current, "0.1.19");
                assert_eq!(latest, "0.1.20");
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn runtime_emits_errors() {
        let runtime = UpdateCheckRuntimeHandle::new_with_checker(|| Err("boom".to_string()))
            .expect("start runtime");

        let event = runtime
            .event_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("receive event");

        match event {
            UpdateCheckRuntimeEvent::Ready(Err(err)) => assert_eq!(err, "boom"),
            other => panic!("unexpected event: {other:?}"),
        }
    }
}
