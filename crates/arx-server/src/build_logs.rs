//! Per-deployment build-log capture, storage, and live fan-out.
//!
//! Unlike runtime container logs — which Docker itself retains and `docker logs`
//! replays — `docker build` output is a one-shot stream that nobody keeps. So
//! arx captures it during the build and persists it to
//! `<build_logs_dir>/<deployment_id>.log`, one file per deployment. That file is
//! the durable record used for after-the-fact retrieval (including past
//! deployments).
//!
//! For live streaming during an in-flight build, a per-deployment
//! [`tokio::sync::broadcast`] channel (held in [`BuildLogHub`]) fans each
//! captured line to any connected subscribers. The same sink both appends to the
//! file and broadcasts, so the file and the live stream never diverge.

use arx_core::ids::DeploymentId;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::io::AsyncWriteExt;
use tokio::sync::broadcast;

/// One captured build-log line, delivered live to subscribers.
#[derive(Clone, Debug)]
pub enum BuildLogEvent {
    /// A build-log line (newline already stripped).
    Line(String),
    /// The build finished; no more lines will follow. Carries success so a
    /// live viewer can render an outcome without polling the deployment status.
    End { success: bool },
}

/// Broadcast capacity per in-flight deployment. A subscriber that lags beyond
/// this many buffered lines is told to re-fetch (see the SSE endpoint) rather
/// than silently losing lines.
const BROADCAST_CAPACITY: usize = 2048;

/// Per-deployment broadcast senders for in-flight builds. An entry exists only
/// while a build is running; it is removed once the terminal event is sent.
#[derive(Clone, Default)]
pub struct BuildLogHub {
    inner: Arc<Mutex<HashMap<DeploymentId, broadcast::Sender<BuildLogEvent>>>>,
}

impl BuildLogHub {
    pub fn new() -> Self {
        Self::default()
    }

    /// Subscribe to a build's live event stream, if it is currently in flight.
    /// Returns `None` when no build is running for that deployment (the caller
    /// should then fall back to the stored file, which is already complete).
    pub fn subscribe(&self, id: DeploymentId) -> Option<broadcast::Receiver<BuildLogEvent>> {
        let map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        map.get(&id).map(|tx| tx.subscribe())
    }

    fn open(&self, id: DeploymentId) -> broadcast::Sender<BuildLogEvent> {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        map.entry(id)
            .or_insert_with(|| broadcast::channel(BROADCAST_CAPACITY).0)
            .clone()
    }

    fn close(&self, id: DeploymentId) {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        map.remove(&id);
    }
}

/// File-backed store for a deployment's build log.
#[derive(Clone)]
pub struct BuildLogStore {
    dir: PathBuf,
}

impl BuildLogStore {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// `<dir>/<deployment_id>.log`. The filename is a server-minted UUID, never
    /// user input, so there is no path-traversal surface.
    fn path(&self, id: DeploymentId) -> PathBuf {
        self.dir.join(format!("{}.log", id.as_uuid()))
    }

    /// Read the whole stored log, or `None` if no file exists yet.
    pub async fn read(&self, id: DeploymentId) -> Option<String> {
        tokio::fs::read_to_string(self.path(id)).await.ok()
    }

    /// Best-effort removal, used when a deployment is deleted.
    pub async fn remove(&self, id: DeploymentId) {
        let _ = tokio::fs::remove_file(self.path(id)).await;
    }
}

/// A live capture session: owns the append-mode file handle and the broadcast
/// sender for one in-flight build. Created via [`BuildLogWriter::begin`]; every
/// captured line is written to both sinks; [`BuildLogWriter::finish`] emits the
/// terminal event and drops the hub entry.
pub struct BuildLogWriter {
    id: DeploymentId,
    hub: BuildLogHub,
    tx: broadcast::Sender<BuildLogEvent>,
    file: Option<tokio::fs::File>,
}

impl BuildLogWriter {
    /// Truncate any previous log for this deployment and start a fresh capture.
    pub async fn begin(
        store: &BuildLogStore,
        hub: &BuildLogHub,
        id: DeploymentId,
    ) -> std::io::Result<Self> {
        tokio::fs::create_dir_all(&store.dir).await?;
        let path = store.path(id);
        let mut opts = tokio::fs::OpenOptions::new();
        opts.create(true).write(true).truncate(true);
        // tokio's OpenOptions exposes `mode` as an inherent method on unix.
        #[cfg(unix)]
        opts.mode(0o600);
        let file = opts.open(&path).await?;
        Ok(Self {
            id,
            hub: hub.clone(),
            tx: hub.open(id),
            file: Some(file),
        })
    }

    /// Append one captured line to the file and broadcast it live. Failures to
    /// write the file are ignored so a capture I/O error never aborts a build.
    pub async fn append(&mut self, line: &str) {
        if let Some(file) = self.file.as_mut() {
            if file.write_all(line.as_bytes()).await.is_err()
                || file.write_all(b"\n").await.is_err()
            {
                // Give up on the file for the rest of this build; keep streaming.
                self.file = None;
            }
        }
        let _ = self.tx.send(BuildLogEvent::Line(line.to_string()));
    }

    /// Emit the terminal event and remove the hub entry so future subscribers
    /// fall back to the now-complete file.
    pub async fn finish(mut self, success: bool) {
        if let Some(mut file) = self.file.take() {
            let _ = file.flush().await;
        }
        let _ = self.tx.send(BuildLogEvent::End { success });
        self.hub.close(self.id);
    }
}

/// Best-effort cleanup of stored build logs for a set of deployment ids,
/// used when a service (and its deployments) is deleted.
pub async fn remove_many(dir: &Path, ids: impl IntoIterator<Item = DeploymentId>) {
    let store = BuildLogStore::new(dir.to_path_buf());
    for id in ids {
        store.remove(id).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unique temp dir under the OS temp root, removed on drop. Avoids adding
    /// a `tempfile` dev-dependency to arx-server just for these tests.
    struct TmpDir(PathBuf);
    impl TmpDir {
        fn new() -> Self {
            let p =
                std::env::temp_dir().join(format!("arx-buildlog-test-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&p).unwrap();
            Self(p)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TmpDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[tokio::test]
    async fn append_persists_and_broadcasts_then_ends() {
        let dir = TmpDir::new();
        let store = BuildLogStore::new(dir.path().to_path_buf());
        let hub = BuildLogHub::new();
        let id = DeploymentId::new();

        let mut sub = hub.subscribe(id);
        assert!(sub.is_none(), "no session yet");

        let mut w = BuildLogWriter::begin(&store, &hub, id).await.unwrap();
        sub = hub.subscribe(id);
        let mut rx = sub.expect("in-flight session should be subscribable");

        w.append("line one").await;
        w.append("line two").await;

        assert!(matches!(rx.recv().await.unwrap(), BuildLogEvent::Line(l) if l == "line one"));
        assert!(matches!(rx.recv().await.unwrap(), BuildLogEvent::Line(l) if l == "line two"));

        w.finish(true).await;
        assert!(matches!(
            rx.recv().await.unwrap(),
            BuildLogEvent::End { success: true }
        ));

        // File persisted for after-the-fact retrieval.
        let stored = store.read(id).await.unwrap();
        assert_eq!(stored, "line one\nline two\n");

        // Session removed after finish.
        assert!(hub.subscribe(id).is_none());
    }

    #[tokio::test]
    async fn remove_deletes_the_file() {
        let dir = TmpDir::new();
        let store = BuildLogStore::new(dir.path().to_path_buf());
        let id = DeploymentId::new();
        let mut w = BuildLogWriter::begin(&store, &BuildLogHub::new(), id)
            .await
            .unwrap();
        w.append("x").await;
        w.finish(false).await;
        assert!(store.read(id).await.is_some());
        store.remove(id).await;
        assert!(store.read(id).await.is_none());
    }
}
