//! Onboard DataFlash log listing and download, as a service next to [`LinkManager`].
//!
//! MAVFTP is preferred: `/APM/LOGS` gives the real file names (`00000042.BIN`) and sizes, and
//! burst reads are fast. Without MAVFTP the classic LOG_* messages are used. Their `id` is a
//! position counted from the oldest log (ArduPilot `AP_Logger_File::find_oldest_log` and
//! `log_num_from_list_entry`: id 1 is the oldest file), *not* the file number, so an FTP path is
//! never built from it: each [`LogEntry`] carries the [`LogSource`] it was listed from.
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use control_station_mavlink::{
    dialect::{LOG_REQUEST_DATA_DATA, LOG_REQUEST_END_DATA, LOG_REQUEST_LIST_DATA},
    MavMessage, Packet,
};
use control_station_vehicle_state::VehicleId;
use serde::{Deserialize, Serialize};
use tokio::{
    sync::{broadcast, Mutex as AsyncMutex},
    time::{timeout_at, Instant},
};

use crate::{FtpService, FtpServiceError, FtpTiming, LinkError, LinkManager};

/// Where ArduPilot keeps its logs on the SD card (`HAL_BOARD_LOG_DIRECTORY` on ChibiOS).
pub const LOG_DIR: &str = "/APM/LOGS";
/// Bytes carried by one LOG_DATA message.
pub const LOG_DATA_LEN: usize = 90;

#[derive(Debug, Clone, Copy)]
pub struct LogTiming {
    /// LOG_ENTRY silence after which missing entries are requested again.
    pub list_idle: Duration,
    /// Rounds in a row without anything new before a listing or download gives up.
    pub tries: u32,
    /// LOG_DATA silence after which the first missing chunk is requested again.
    pub data_idle: Duration,
    /// LOG_DATA chunks asked for in one LOG_REQUEST_DATA.
    pub window_chunks: usize,
    /// Minimum time between two progress events during a transfer.
    pub progress_interval: Duration,
    /// Timing of the internal MAVFTP client. Short tries keep the probe quick on vehicles
    /// without MAVFTP, since its failure is what selects the LOG_* fallback.
    pub ftp: FtpTiming,
}

impl Default for LogTiming {
    fn default() -> Self {
        Self {
            list_idle: Duration::from_millis(1000),
            tries: 5,
            data_idle: Duration::from_millis(1000),
            window_chunks: 512,
            progress_interval: Duration::from_millis(100),
            ftp: FtpTiming {
                request_timeout: Duration::from_millis(1000),
                tries: 3,
                ..FtpTiming::default()
            },
        }
    }
}

/// How a log is fetched; the UI sends it back unchanged inside the [`LogEntry`] to download.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum LogSource {
    /// A file read through MAVFTP.
    Ftp { path: String },
    /// A LOG_ENTRY id (a list position, not the file number).
    Mavlink { id: u16 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    /// File name (`00000042.BIN`) over MAVFTP, `Log <id>` over LOG_*.
    pub name: String,
    /// Bytes (LOG_ENTRY sizes may be approximate).
    pub size: u64,
    /// Seconds since the Unix epoch, when the vehicle knows it.
    pub time_utc: Option<u32>,
    pub source: LogSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogMethod {
    Ftp,
    Mavlink,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogProgress {
    /// `list` or `download`.
    pub op: String,
    /// The log being downloaded (empty for `list`).
    pub name: String,
    /// Bytes received (entries for `list`).
    pub done: u64,
    pub total: u64,
    pub method: LogMethod,
    /// Bytes per second since the operation started.
    pub rate: f64,
    pub finished: bool,
    pub error: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum LogError {
    #[error("{0}")]
    Link(#[from] LinkError),
    #[error("another log operation is already in progress")]
    Busy,
    #[error("cancelled")]
    Cancelled,
    #[error("no answer from the vehicle to {0}")]
    Timeout(&'static str),
    #[error("{0}")]
    Ftp(FtpServiceError),
}

impl From<FtpServiceError> for LogError {
    fn from(error: FtpServiceError) -> Self {
        match error {
            FtpServiceError::Cancelled => LogError::Cancelled,
            other => LogError::Ftp(other),
        }
    }
}

struct Inner {
    link: LinkManager,
    ftp: FtpService,
    progress: broadcast::Sender<LogProgress>,
    op_lock: AsyncMutex<()>,
    cancel: AtomicBool,
    timing: LogTiming,
}

impl Inner {
    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
}

#[derive(Clone)]
pub struct LogService {
    inner: Arc<Inner>,
}

/// The running operation, shared by its progress reports.
#[derive(Clone)]
struct Op {
    name: &'static str,
    log: String,
    vehicle: VehicleId,
    started: Instant,
    method: Arc<Mutex<LogMethod>>,
}

impl Op {
    fn method(&self) -> LogMethod {
        *self.method.lock().expect("log method")
    }

    fn set_method(&self, method: LogMethod) {
        *self.method.lock().expect("log method") = method;
    }

    fn rate(&self, bytes: u64) -> f64 {
        let seconds = self.started.elapsed().as_secs_f64();
        if seconds > 0.0 {
            bytes as f64 / seconds
        } else {
            0.0
        }
    }
}

type Packets = broadcast::Receiver<Arc<Packet>>;

impl LogService {
    pub fn new(link: LinkManager) -> Self {
        Self::build(link, None, LogTiming::default())
    }

    pub fn with_timing(self, timing: LogTiming) -> Self {
        Self::build(self.inner.link.clone(), None, timing)
    }

    /// Uses `ftp` (for example the one the file browser uses, so the two never run MAVFTP
    /// sessions at once) instead of an internal client; its own timing then applies.
    pub fn with_ftp(self, ftp: FtpService) -> Self {
        Self::build(self.inner.link.clone(), Some(ftp), self.inner.timing)
    }

    fn build(link: LinkManager, ftp: Option<FtpService>, timing: LogTiming) -> Self {
        let (progress, _) = broadcast::channel(256);
        let ftp = ftp.unwrap_or_else(|| FtpService::new(link.clone()).with_timing(timing.ftp));
        Self {
            inner: Arc::new(Inner {
                link,
                ftp,
                progress,
                op_lock: AsyncMutex::new(()),
                cancel: AtomicBool::new(false),
                timing,
            }),
        }
    }

    pub fn subscribe_progress(&self) -> broadcast::Receiver<LogProgress> {
        self.inner.progress.subscribe()
    }

    /// Asks the running listing or download to stop at the next opportunity.
    pub fn cancel(&self) {
        self.inner.cancel.store(true, Ordering::SeqCst);
        self.inner.ftp.cancel();
    }

    fn check_cancel(&self) -> Result<(), LogError> {
        if self.inner.cancelled() {
            Err(LogError::Cancelled)
        } else {
            Ok(())
        }
    }

    fn emit(&self, op: &Op, done: u64, total: u64, finished: bool, error: Option<String>) {
        let rate = if op.name == "download" {
            op.rate(done)
        } else {
            0.0
        };
        let _ = self.inner.progress.send(LogProgress {
            op: op.name.to_string(),
            name: op.log.clone(),
            done,
            total,
            method: op.method(),
            rate,
            finished,
            error,
        });
    }

    /// Runs `body` as the one active operation, reporting start and finish. `amount` gives the
    /// final `done`/`total` of a successful result.
    async fn run<T, F, Fut>(
        &self,
        name: &'static str,
        log: &str,
        method: LogMethod,
        amount: fn(&T) -> u64,
        body: F,
    ) -> Result<T, LogError>
    where
        F: FnOnce(Op) -> Fut,
        Fut: std::future::Future<Output = Result<T, LogError>>,
    {
        let _guard = self.inner.op_lock.try_lock().map_err(|_| LogError::Busy)?;
        self.inner.cancel.store(false, Ordering::SeqCst);
        let (vehicle, _) = self.inner.link.selected_vehicle()?;
        let op = Op {
            name,
            log: log.to_string(),
            vehicle,
            started: Instant::now(),
            method: Arc::new(Mutex::new(method)),
        };
        self.emit(&op, 0, 0, false, None);
        let result = body(op.clone()).await;
        match &result {
            Ok(value) => {
                let n = amount(value);
                self.emit(&op, n, n, true, None);
            }
            Err(error) => self.emit(&op, 0, 0, true, Some(error.to_string())),
        }
        result
    }

    /// The vehicle's logs, oldest first. MAVFTP's `/APM/LOGS` listing is used when it works
    /// and is not empty; otherwise LOG_REQUEST_LIST. A Replay link answers with no logs.
    pub async fn list(&self) -> Result<Vec<LogEntry>, LogError> {
        self.run(
            "list",
            "",
            LogMethod::Ftp,
            |entries: &Vec<LogEntry>| entries.len() as u64,
            |op| async move {
                match self.list_ftp().await {
                    Ok(entries) if !entries.is_empty() => return Ok(entries),
                    Err(FtpServiceError::Cancelled) => return Err(LogError::Cancelled),
                    // empty, missing directory or no MAVFTP: ask with LOG_* instead
                    _ => {}
                }
                self.check_cancel()?;
                op.set_method(LogMethod::Mavlink);
                let result = self.list_mavlink(&op).await;
                self.send_end(&op);
                result
            },
        )
        .await
    }

    async fn list_ftp(&self) -> Result<Vec<LogEntry>, FtpServiceError> {
        let mut entries: Vec<LogEntry> = self
            .inner
            .ftp
            .list(LOG_DIR)
            .await?
            .into_iter()
            .filter(|e| !e.is_dir && e.name.to_ascii_uppercase().ends_with(".BIN"))
            .map(|e| LogEntry {
                source: LogSource::Ftp {
                    path: format!("{LOG_DIR}/{}", e.name),
                },
                name: e.name,
                size: e.size,
                time_utc: e.modified.filter(|&t| t != 0),
            })
            .collect();
        entries.sort_by_key(|e| (log_number(&e.name), e.name.clone()));
        Ok(entries)
    }

    async fn list_mavlink(&self, op: &Op) -> Result<Vec<LogEntry>, LogError> {
        let timing = self.inner.timing;
        let mut packets = self.inner.link.subscribe_packets();
        // first id and count, from the first LOG_ENTRY (an answer to the full request)
        let mut expected: Option<(u16, u16)> = None;
        let mut found: BTreeMap<u16, LogEntry> = BTreeMap::new();
        let mut stalled = 0;
        let mut request = (0u16, u16::MAX);
        loop {
            self.check_cancel()?;
            self.send(&MavMessage::LOG_REQUEST_LIST(LOG_REQUEST_LIST_DATA {
                target_system: op.vehicle.system_id,
                target_component: op.vehicle.component_id,
                start: request.0,
                end: request.1,
            }))?;
            let before = found.len();
            let mut deadline = Instant::now() + timing.list_idle;
            while let Some(message) = next_message(&mut packets, op.vehicle, deadline).await {
                self.check_cancel()?;
                let MavMessage::LOG_ENTRY(entry) = message else {
                    continue;
                };
                deadline = Instant::now() + timing.list_idle;
                if entry.num_logs == 0 {
                    return Ok(Vec::new());
                }
                let (first, count) = *expected.get_or_insert_with(|| {
                    // ArduPilot numbers 1..=num_logs and echoes the clamped `end` as
                    // last_log_num; a stack counting from 0 reports last_log_num = num_logs - 1.
                    let first =
                        u16::from(entry.last_log_num.checked_add(1) != Some(entry.num_logs));
                    (first, entry.num_logs)
                });
                if entry.id < first || u32::from(entry.id) >= u32::from(first) + u32::from(count) {
                    continue;
                }
                found.insert(
                    entry.id,
                    LogEntry {
                        name: format!("Log {}", entry.id),
                        size: u64::from(entry.size),
                        time_utc: (entry.time_utc != 0).then_some(entry.time_utc),
                        source: LogSource::Mavlink { id: entry.id },
                    },
                );
                self.emit(op, found.len() as u64, u64::from(count), false, None);
                if found.len() == usize::from(count) {
                    break;
                }
            }
            if let Some((first, count)) = expected {
                let missing: Vec<u16> = (first..)
                    .take(usize::from(count))
                    .filter(|id| !found.contains_key(id))
                    .collect();
                match (missing.first(), missing.last()) {
                    (Some(&low), Some(&high)) => request = (low, high),
                    _ => return Ok(found.into_values().collect()),
                }
            }
            if found.len() == before {
                stalled += 1;
                if stalled >= timing.tries {
                    return Err(LogError::Timeout("LOG_REQUEST_LIST"));
                }
            } else {
                stalled = 0;
            }
        }
    }

    /// Downloads one log listed by [`list`](Self::list).
    pub async fn download(&self, entry: &LogEntry) -> Result<Vec<u8>, LogError> {
        let method = match entry.source {
            LogSource::Ftp { .. } => LogMethod::Ftp,
            LogSource::Mavlink { .. } => LogMethod::Mavlink,
        };
        self.run(
            "download",
            &entry.name,
            method,
            |bytes: &Vec<u8>| bytes.len() as u64,
            |op| async move {
                match &entry.source {
                    LogSource::Ftp { path } => self.download_ftp(&op, path).await,
                    LogSource::Mavlink { id } => {
                        let result = self.download_mavlink(&op, *id, entry.size).await;
                        // always: ArduPilot keeps streaming LOG_DATA until told to stop
                        self.send_end(&op);
                        result
                    }
                }
            },
        )
        .await
    }

    async fn download_ftp(&self, op: &Op, path: &str) -> Result<Vec<u8>, LogError> {
        let mut ftp_progress = self.inner.ftp.subscribe_progress();
        let relay = {
            let inner = self.inner.clone();
            let op = op.clone();
            let path = path.to_string();
            tokio::spawn(async move {
                loop {
                    match ftp_progress.recv().await {
                        Ok(p) if p.op == "read" && p.path == path && !p.finished => {
                            // FtpService clears its cancel flag when an operation starts, so
                            // a cancel that came just before the read is passed on again here.
                            if inner.cancelled() {
                                inner.ftp.cancel();
                            }
                            let _ = inner.progress.send(LogProgress {
                                op: op.name.to_string(),
                                name: op.log.clone(),
                                done: p.done,
                                total: p.total,
                                method: LogMethod::Ftp,
                                rate: op.rate(p.done),
                                finished: false,
                                error: None,
                            });
                        }
                        Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                        Err(broadcast::error::RecvError::Closed) => return,
                    }
                }
            })
        };
        let result = async {
            self.check_cancel()?;
            let bytes = self.inner.ftp.read(path).await?;
            self.check_cancel()?;
            Ok(bytes)
        }
        .await;
        relay.abort();
        result
    }

    async fn download_mavlink(&self, op: &Op, id: u16, size: u64) -> Result<Vec<u8>, LogError> {
        let timing = self.inner.timing;
        let window = timing.window_chunks.max(1);
        let mut total = usize::try_from(size).unwrap_or(usize::MAX);
        let mut buf = vec![0u8; total];
        let mut have = vec![false; total.div_ceil(LOG_DATA_LEN)];
        let mut received = 0usize;
        let mut stalled = 0;
        let mut last_emit = Instant::now();
        let mut packets = self.inner.link.subscribe_packets();
        while let Some(first) = have.iter().position(|h| !h) {
            self.check_cancel()?;
            // the run of missing chunks starting at `first`, at most one window long
            let end = have[first..]
                .iter()
                .position(|h| *h)
                .map_or(have.len(), |n| first + n)
                .min(first + window);
            let ofs = first * LOG_DATA_LEN;
            let count = (end * LOG_DATA_LEN).min(total) - ofs;
            self.send(&MavMessage::LOG_REQUEST_DATA(LOG_REQUEST_DATA_DATA {
                target_system: op.vehicle.system_id,
                target_component: op.vehicle.component_id,
                id,
                ofs: ofs as u32,
                count: count as u32,
            }))?;
            let before = received;
            let mut deadline = Instant::now() + timing.data_idle;
            while let Some(message) = next_message(&mut packets, op.vehicle, deadline).await {
                self.check_cancel()?;
                let MavMessage::LOG_DATA(data) = message else {
                    continue;
                };
                if data.id != id {
                    continue;
                }
                deadline = Instant::now() + timing.data_idle;
                let at = data.ofs as usize;
                if !at.is_multiple_of(LOG_DATA_LEN) {
                    continue;
                }
                let len = usize::from(data.count).min(LOG_DATA_LEN);
                if len < LOG_DATA_LEN && at + len < total {
                    // a short (or empty) chunk is the end of the log: the listed size was high
                    total = at + len;
                    buf.truncate(total);
                    have.truncate(total.div_ceil(LOG_DATA_LEN));
                    received = have
                        .iter()
                        .enumerate()
                        .filter(|(_, h)| **h)
                        .map(|(i, _)| chunk_len(i, total))
                        .sum();
                }
                let index = at / LOG_DATA_LEN;
                if index < have.len() && !have[index] {
                    let len = chunk_len(index, total).min(len);
                    buf[at..at + len].copy_from_slice(&data.data[..len]);
                    have[index] = true;
                    received += len;
                }
                if last_emit.elapsed() >= timing.progress_interval {
                    last_emit = Instant::now();
                    self.emit(op, received as u64, total as u64, false, None);
                }
                if have[first.min(have.len())..end.min(have.len())]
                    .iter()
                    .all(|h| *h)
                {
                    break;
                }
            }
            if received == before && have.get(first).is_some_and(|h| !h) {
                stalled += 1;
                if stalled >= timing.tries {
                    return Err(LogError::Timeout("LOG_REQUEST_DATA"));
                }
            } else {
                stalled = 0;
            }
        }
        buf.truncate(total);
        Ok(buf)
    }

    fn send(&self, message: &MavMessage) -> Result<(), LogError> {
        Ok(self.inner.link.send(message)?)
    }

    /// LOG_REQUEST_END, best effort: lets the vehicle stop streaming and resume logging.
    fn send_end(&self, op: &Op) {
        let _ = self.send(&MavMessage::LOG_REQUEST_END(LOG_REQUEST_END_DATA {
            target_system: op.vehicle.system_id,
            target_component: op.vehicle.component_id,
        }));
    }
}

/// Bytes in chunk `index` of a log of `total` bytes.
fn chunk_len(index: usize, total: usize) -> usize {
    total.saturating_sub(index * LOG_DATA_LEN).min(LOG_DATA_LEN)
}

/// The number in an ArduPilot log name (`00000042.BIN` -> 42); other names sort last.
fn log_number(name: &str) -> u64 {
    name.split('.')
        .next()
        .and_then(|stem| stem.parse().ok())
        .unwrap_or(u64::MAX)
}

/// The next message from `vehicle` before `deadline`.
async fn next_message(
    packets: &mut Packets,
    vehicle: VehicleId,
    deadline: Instant,
) -> Option<MavMessage> {
    loop {
        match timeout_at(deadline, packets.recv()).await {
            Err(_) | Ok(Err(broadcast::error::RecvError::Closed)) => return None,
            Ok(Err(broadcast::error::RecvError::Lagged(_))) => continue,
            Ok(Ok(packet)) => {
                if packet.system_id() == vehicle.system_id
                    && packet.component_id() == vehicle.component_id
                {
                    return Some(packet.message.clone());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn log_entries_round_trip_the_ui_json() {
        let ftp = LogEntry {
            name: "00000006.BIN".into(),
            size: 1234,
            time_utc: Some(1_700_000_000),
            source: LogSource::Ftp {
                path: "/APM/LOGS/00000006.BIN".into(),
            },
        };
        let value = serde_json::to_value(&ftp).unwrap();
        assert_eq!(
            value,
            json!({
                "name": "00000006.BIN",
                "size": 1234,
                "timeUtc": 1_700_000_000,
                "source": {"type": "ftp", "path": "/APM/LOGS/00000006.BIN"}
            })
        );
        assert_eq!(serde_json::from_value::<LogEntry>(value).unwrap(), ftp);

        let mavlink = json!({
            "name": "Log 2",
            "size": 90,
            "timeUtc": null,
            "source": {"type": "mavlink", "id": 2}
        });
        assert_eq!(
            serde_json::from_value::<LogEntry>(mavlink.clone()).unwrap(),
            LogEntry {
                name: "Log 2".into(),
                size: 90,
                time_utc: None,
                source: LogSource::Mavlink { id: 2 },
            }
        );
        assert_eq!(
            serde_json::to_value(serde_json::from_value::<LogEntry>(mavlink.clone()).unwrap())
                .unwrap(),
            mavlink
        );
    }

    #[test]
    fn unknown_log_source_is_rejected() {
        assert!(serde_json::from_value::<LogSource>(json!({"type": "usb", "id": 1})).is_err());
    }

    #[test]
    fn progress_uses_camel_case_and_lowercase_method() {
        let value = serde_json::to_value(LogProgress {
            op: "download".into(),
            name: "Log 1".into(),
            done: 900,
            total: 1800,
            method: LogMethod::Mavlink,
            rate: 450.0,
            finished: false,
            error: None,
        })
        .unwrap();
        assert_eq!(
            value,
            json!({
                "op": "download",
                "name": "Log 1",
                "done": 900,
                "total": 1800,
                "method": "mavlink",
                "rate": 450.0,
                "finished": false,
                "error": null
            })
        );
        assert_eq!(serde_json::to_value(LogMethod::Ftp).unwrap(), "ftp");
    }

    #[test]
    fn log_names_sort_by_number() {
        assert_eq!(log_number("00000042.BIN"), 42);
        assert_eq!(log_number("LASTLOG.TXT"), u64::MAX);
        assert_eq!(chunk_len(0, 200), 90);
        assert_eq!(chunk_len(2, 200), 20);
        assert_eq!(chunk_len(3, 200), 0);
    }
}
