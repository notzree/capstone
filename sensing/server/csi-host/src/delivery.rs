//! Unix-socket Arrow delivery, downstream of the recorder's lossy subscription.
//! No Python client participates in the raw recording backpressure path.
use std::fs;
use std::io::{self, Read};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};

use arrow_array::RecordBatch;
use arrow_ipc::writer::StreamWriter;
use serde_json::json;
use tokio::sync::mpsc;

use crate::error::IoContext;
use crate::export::{self, Encoder, Row};
use crate::geometry::Geometry;
use crate::manifest::Manifest;
use crate::{Error, Record};

const CLIENT_BATCHES: usize = 8;
const MAX_CLIENTS: usize = 16;

/// Memory health is available in every mode; disk sidecars are optional.
#[derive(Default)]
pub struct DeliveryHealth {
    pub primary_ready: AtomicBool,
    pub primary_failed: AtomicBool,
    pub status: Mutex<serde_json::Value>,
}

/// Binding never removes an existing socket/file. Drop removes only our socket.
pub struct Publisher {
    listener: UnixListener,
    path: PathBuf,
    identity: (u64, u64),
    flush_interval: Duration,
    primary_handshake: bool,
    metadata: std::collections::HashMap<String, String>,
}

impl Drop for Publisher {
    fn drop(&mut self) {
        if fs::symlink_metadata(&self.path).is_ok_and(|m| (m.dev(), m.ino()) == self.identity) {
            let _ = fs::remove_file(&self.path);
        }
    }
}

impl Publisher {
    pub fn bind(path: &Path) -> Result<Self, Error> {
        let listener = UnixListener::bind(path).at(path)?;
        let metadata = fs::symlink_metadata(path).at(path)?;
        let publisher = Self {
            listener,
            path: path.to_owned(),
            identity: (metadata.dev(), metadata.ino()),
            flush_interval: Duration::from_millis(100),
            primary_handshake: false,
            metadata: Default::default(),
        };
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).at(path)?;
        publisher.listener.set_nonblocking(true).at(path)?;
        Ok(publisher)
    }

    /// Lower intervals trade smaller batches for more IPC/processing overhead.
    pub fn with_flush_interval_ms(mut self, ms: u64) -> Result<Self, Error> {
        if !(5..=1000).contains(&ms) {
            return Err(Error::Config(
                "Arrow flush interval must be 5..=1000 ms".into(),
            ));
        }
        self.flush_interval = Duration::from_millis(ms);
        Ok(self)
    }

    pub fn with_primary_handshake(
        mut self,
        metadata: std::collections::HashMap<String, String>,
    ) -> Self {
        self.primary_handshake = true;
        self.metadata = metadata;
        self
    }

    /// Drain input until the recorder closes it; finish client streams on shutdown.
    /// The recorder's subscriber-drop counter measures missing inputs here, while
    /// client_batch_drops measures completed batches dropped for stalled readers.
    pub fn spawn(
        self,
        input: mpsc::Receiver<Arc<Record>>,
        input_drops: Arc<AtomicU64>,
        manifest: Manifest,
        geometry: Option<Geometry>,
        status_dir: PathBuf,
    ) -> io::Result<thread::JoinHandle<Result<(), Error>>> {
        self.spawn_optional(
            input,
            input_drops,
            manifest,
            geometry,
            Some(status_dir),
            Arc::new(DeliveryHealth::default()),
        )
    }

    pub fn spawn_optional(
        self,
        input: mpsc::Receiver<Arc<Record>>,
        input_drops: Arc<AtomicU64>,
        manifest: Manifest,
        geometry: Option<Geometry>,
        status_dir: Option<PathBuf>,
        health: Arc<DeliveryHealth>,
    ) -> io::Result<thread::JoinHandle<Result<(), Error>>> {
        thread::Builder::new()
            .name("csi-arrow-publisher".into())
            .spawn(move || {
                let result = self.run(
                    input,
                    input_drops,
                    &manifest,
                    geometry,
                    status_dir.as_deref(),
                    &health,
                );
                if let Err(error) = &result {
                    health.primary_failed.store(true, Ordering::Release);
                    let status = json!({"complete":false,"error":error.to_string()});
                    *health.status.lock().expect("delivery health") = status.clone();
                    if let Some(dir) = &status_dir {
                        let _ = fs::write(dir.join("arrow-status.json"), status.to_string());
                    }
                }
                result
            })
    }

    fn run(
        &self,
        mut input: mpsc::Receiver<Arc<Record>>,
        input_drops: Arc<AtomicU64>,
        manifest: &Manifest,
        geometry: Option<Geometry>,
        status_dir: Option<&Path>,
        health: &Arc<DeliveryHealth>,
    ) -> Result<(), Error> {
        let mut metadata = export::metadata(manifest);
        metadata.insert(
            "csi.geometry".into(),
            serde_json::to_string(&geometry).map_err(|e| Error::Config(e.to_string()))?,
        );
        metadata.insert(
            "csi.delivery".into(),
            "live; bounded lossy subscriptions; later clients receive future batches only; query final controller health (file runs also store arrow-status.json)".into(),
        );
        metadata.extend(self.metadata.clone());
        let schema = export::batch(&[], metadata.clone())?.schema();
        let mut clients: Vec<(bool, mpsc::Sender<RecordBatch>)> = Vec::new();
        let mut workers = Vec::new();
        let batch_drops = Arc::new(AtomicU64::new(0));
        let write_failures = Arc::new(AtomicU64::new(0));
        let mut connections = 0u64;
        let mut rejected = 0u64;
        let mut batches = 0u64;
        let mut encoder = Encoder::live();
        let mut rows: Vec<Row> = Vec::with_capacity(export::BATCH_ROWS);
        let mut last_flush = Instant::now();
        let mut last_status = Instant::now() - Duration::from_secs(1);
        let mut done = false;
        loop {
            clients.retain(|(primary, tx)| {
                if *primary && tx.is_closed() {
                    health.primary_failed.store(true, Ordering::Release);
                }
                !tx.is_closed()
            });
            // Reap finished client threads; retained threads are capped by MAX_CLIENTS.
            let mut i = 0;
            while i < workers.len() {
                let worker: &thread::JoinHandle<()> = &workers[i];
                if worker.is_finished() {
                    let _ = workers.swap_remove(i).join();
                } else {
                    i += 1;
                }
            }
            // Bound accepts per iteration so connection churn cannot starve input.
            for _ in 0..MAX_CLIENTS {
                match self.listener.accept() {
                    Ok((stream, _)) => {
                        if workers.len() >= MAX_CLIENTS {
                            rejected += 1;
                            continue;
                        }
                        // macOS can inherit O_NONBLOCK from the listening socket.
                        // Arrow's synchronous writer needs blocking write_all semantics.
                        stream.set_nonblocking(false).at(&self.path)?;
                        stream
                            .set_write_timeout(Some(Duration::from_millis(250)))
                            .at(&self.path)?;
                        let primary = self.primary_handshake && connections == 0;
                        let health = health.clone();
                        let mut reader = stream.try_clone().at(&self.path)?;
                        let schema = schema.clone();
                        let errors = write_failures.clone();
                        let (tx, mut rx) = mpsc::channel::<RecordBatch>(CLIENT_BATCHES);
                        workers.push(thread::spawn(move || {
                            let result = (|| -> Result<(), Error> {
                                let mut writer = StreamWriter::try_new(stream, &schema)?;
                                if primary {
                                    reader
                                        .set_read_timeout(Some(Duration::from_secs(10)))
                                        .map_err(|e| Error::Config(e.to_string()))?;
                                    let mut ack = [0; 12];
                                    reader.read_exact(&mut ack).map_err(|e| {
                                        Error::Config(format!("schema acknowledgement: {e}"))
                                    })?;
                                    if &ack != b"CSI-READY/1\n" {
                                        return Err(Error::Config(
                                            "invalid schema acknowledgement".into(),
                                        ));
                                    }
                                    health.primary_ready.store(true, Ordering::Release);
                                    reader
                                        .set_read_timeout(Some(Duration::from_millis(5)))
                                        .map_err(|e| Error::Config(e.to_string()))?;
                                }
                                loop {
                                    match rx.try_recv() {
                                        Ok(batch) => writer.write(&batch)?,
                                        Err(mpsc::error::TryRecvError::Disconnected) => break,
                                        Err(mpsc::error::TryRecvError::Empty) => {
                                            if primary {
                                                let mut byte = [0];
                                                match reader.read(&mut byte) {
                                                    Ok(0) => {
                                                        return Err(Error::Config(
                                                            "primary disconnected".into(),
                                                        ));
                                                    }
                                                    Ok(_) => {
                                                        return Err(Error::Config(
                                                            "unexpected primary data".into(),
                                                        ));
                                                    }
                                                    Err(e)
                                                        if matches!(
                                                            e.kind(),
                                                            io::ErrorKind::WouldBlock
                                                                | io::ErrorKind::TimedOut
                                                        ) => {}
                                                    Err(e) => {
                                                        return Err(Error::Config(e.to_string()));
                                                    }
                                                }
                                            } else {
                                                thread::sleep(Duration::from_millis(5));
                                            }
                                        }
                                    }
                                }
                                writer.finish()?;
                                Ok(())
                            })();
                            if result.is_err() {
                                errors.fetch_add(1, Ordering::Relaxed);
                                if primary {
                                    health.primary_failed.store(true, Ordering::Release);
                                }
                            }
                        }));
                        clients.push((primary, tx));
                        connections += 1;
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                    Err(source) => {
                        return Err(Error::Io {
                            path: self.path.clone(),
                            source,
                        });
                    }
                }
            }
            // Bound work before checking accepts and flush deadlines again.
            for _ in 0..export::BATCH_ROWS {
                match input.try_recv() {
                    Ok(record) => {
                        rows.extend(encoder.push(&record));
                        if rows.len() >= export::BATCH_ROWS {
                            break;
                        }
                    }
                    Err(mpsc::error::TryRecvError::Empty) => break,
                    Err(mpsc::error::TryRecvError::Disconnected) => {
                        done = true;
                        break;
                    }
                }
            }
            if !rows.is_empty()
                && (done
                    || rows.len() >= export::BATCH_ROWS
                    || last_flush.elapsed() >= self.flush_interval)
            {
                for batch in export::grouped_batches(&mut rows, &metadata)? {
                    batches += 1;
                    clients.retain(|(primary, client)| match client.try_send(batch.clone()) {
                        Ok(()) => true,
                        Err(_) => {
                            batch_drops.fetch_add(1, Ordering::Relaxed);
                            if *primary {
                                health.primary_failed.store(true, Ordering::Release);
                            }
                            false
                        }
                    });
                }
                last_flush = Instant::now();
            }
            if done || last_status.elapsed() >= Duration::from_secs(1) {
                let status = json!({"schema":"csi-arrow-status/1","session_id":manifest.session_id,"complete":done,
                    "input_record_drops":input_drops.load(Ordering::Relaxed),
                    "client_batch_drops":batch_drops.load(Ordering::Relaxed),
                    "client_write_failures":write_failures.load(Ordering::Relaxed),
                    "connections":connections,"active_clients":clients.len(),"rejected_connections":rejected,
                    "batches":batches,"summary":encoder.summary,"cache_evictions":encoder.cache_evictions,
                    "clock_capacity_rejects":encoder.clocks.capacity_rejects,
                    "input_closed":done,"socket":self.path,"client_batch_capacity":CLIENT_BATCHES,
                    "flush_interval_ms":self.flush_interval.as_millis()});
                *health.status.lock().expect("delivery health") = status.clone();
                if let Some(dir) = status_dir {
                    store_status(dir, &status)?;
                }
                last_status = Instant::now();
            }
            if done {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        drop(clients);
        for worker in workers {
            let _ = worker.join();
        }
        let mut status = health.status.lock().expect("delivery health");
        status["active_clients"] = 0.into();
        status["client_write_failures"] = write_failures.load(Ordering::Relaxed).into();
        if let Some(dir) = status_dir {
            store_status(dir, &status)?;
        }
        Ok(())
    }
}

fn store_status(dir: &Path, status: &serde_json::Value) -> Result<(), Error> {
    let path = dir.join("arrow-status.json");
    let tmp = dir.join("arrow-status.json.tmp");
    fs::write(
        &tmp,
        serde_json::to_vec_pretty(status).map_err(|e| Error::Config(e.to_string()))?,
    )
    .at(&tmp)?;
    fs::rename(tmp, &path).at(&path)
}
