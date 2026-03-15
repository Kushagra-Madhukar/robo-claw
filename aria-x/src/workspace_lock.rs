#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WorkspaceLockSnapshot {
    pub workspace_key: String,
    pub active_holders: usize,
    pub waiting_runs: usize,
    pub current_holder: Option<String>,
    pub updated_at_us: u64,
}

#[derive(Debug)]
struct WorkspaceLockEntry {
    semaphore: Arc<tokio::sync::Semaphore>,
    active_holders: std::sync::atomic::AtomicUsize,
    waiting_runs: std::sync::atomic::AtomicUsize,
    current_holder: std::sync::Mutex<Option<String>>,
    updated_at_us: std::sync::atomic::AtomicU64,
}

impl WorkspaceLockEntry {
    fn new(now_us: u64) -> Self {
        Self {
            semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
            active_holders: std::sync::atomic::AtomicUsize::new(0),
            waiting_runs: std::sync::atomic::AtomicUsize::new(0),
            current_holder: std::sync::Mutex::new(None),
            updated_at_us: std::sync::atomic::AtomicU64::new(now_us),
        }
    }
}

#[derive(Debug, Clone)]
pub struct WorkspaceLockManager {
    wait_timeout: Duration,
    entries: Arc<dashmap::DashMap<String, Arc<WorkspaceLockEntry>>>,
}

#[derive(Debug)]
pub struct WorkspaceLockGuard {
    entry: Arc<WorkspaceLockEntry>,
    _permit: tokio::sync::OwnedSemaphorePermit,
}

impl Drop for WorkspaceLockGuard {
    fn drop(&mut self) {
        self.entry
            .active_holders
            .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        if let Ok(mut holder) = self.entry.current_holder.lock() {
            *holder = None;
        }
        self.entry
            .updated_at_us
            .store(now_us(), std::sync::atomic::Ordering::SeqCst);
    }
}

impl WorkspaceLockManager {
    pub fn new(wait_timeout: Duration) -> Self {
        Self {
            wait_timeout,
            entries: Arc::new(dashmap::DashMap::new()),
        }
    }

    pub async fn acquire(
        &self,
        workspace_key: impl Into<String>,
        holder_id: impl Into<String>,
    ) -> Result<WorkspaceLockGuard, aria_intelligence::OrchestratorError> {
        let workspace_key = workspace_key.into();
        let holder_id = holder_id.into();
        let current_us = now_us();
        let entry = self
            .entries
            .entry(workspace_key.clone())
            .or_insert_with(|| Arc::new(WorkspaceLockEntry::new(current_us)))
            .clone();
        entry
            .waiting_runs
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        entry
            .updated_at_us
            .store(current_us, std::sync::atomic::Ordering::SeqCst);

        let permit = match tokio::time::timeout(
            self.wait_timeout,
            Arc::clone(&entry.semaphore).acquire_owned(),
        )
        .await
        {
            Ok(Ok(permit)) => permit,
            Ok(Err(_)) => {
                entry
                    .waiting_runs
                    .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                entry
                    .updated_at_us
                    .store(now_us(), std::sync::atomic::Ordering::SeqCst);
                return Err(aria_intelligence::OrchestratorError::ResourceBusy(
                    format!("workspace lock '{}' is unavailable", workspace_key),
                ));
            }
            Err(_) => {
                entry
                    .waiting_runs
                    .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                entry
                    .updated_at_us
                    .store(now_us(), std::sync::atomic::Ordering::SeqCst);
                let current_holder = entry
                    .current_holder
                    .lock()
                    .ok()
                    .and_then(|holder| holder.clone())
                    .unwrap_or_else(|| "unknown".to_string());
                return Err(aria_intelligence::OrchestratorError::ResourceBusy(
                    format!(
                        "workspace '{}' is busy (held by '{}')",
                        workspace_key, current_holder
                    ),
                ));
            }
        };

        entry
            .waiting_runs
            .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        entry
            .active_holders
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if let Ok(mut current_holder) = entry.current_holder.lock() {
            *current_holder = Some(holder_id);
        }
        entry
            .updated_at_us
            .store(now_us(), std::sync::atomic::Ordering::SeqCst);

        Ok(WorkspaceLockGuard {
            entry,
            _permit: permit,
        })
    }

    pub fn snapshot(&self) -> Vec<WorkspaceLockSnapshot> {
        let mut snapshots = self
            .entries
            .iter()
            .map(|entry| WorkspaceLockSnapshot {
                workspace_key: entry.key().clone(),
                active_holders: entry
                    .active_holders
                    .load(std::sync::atomic::Ordering::SeqCst),
                waiting_runs: entry
                    .waiting_runs
                    .load(std::sync::atomic::Ordering::SeqCst),
                current_holder: entry
                    .current_holder
                    .lock()
                    .ok()
                    .and_then(|holder| holder.clone()),
                updated_at_us: entry
                    .updated_at_us
                    .load(std::sync::atomic::Ordering::SeqCst),
            })
            .filter(|snapshot| snapshot.active_holders > 0 || snapshot.waiting_runs > 0)
            .collect::<Vec<_>>();
        snapshots.sort_by(|a, b| a.workspace_key.cmp(&b.workspace_key));
        snapshots
    }
}

fn now_us() -> u64 {
    chrono::Utc::now().timestamp_micros() as u64
}
