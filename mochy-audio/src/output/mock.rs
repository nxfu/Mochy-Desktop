//! The mock backend — exactness-gate and hotplug tests.
//!
//! Not `cfg(test)`: mochy-player's hotplug-resume acceptance test drives
//! it too (the router + core + mock loop is the whole
//! device-removed-mid-song scenario).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc::Sender};

use super::{
    OpenError, OpenedStream, OutputBackend, OutputEvent, OutputFormat, RenderSource, Sharing,
    StreamReport, StreamRequest,
};

/// One mock device: whether it grants exclusive asks exactly.
#[derive(Debug, Clone)]
pub struct MockDevice {
    pub id: String,
    /// `true` = grants the asked rate/format/channels exclusively;
    /// `false` = answers with something else (a shared stream at the
    /// device default).
    pub exact: bool,
}

#[derive(Default)]
pub(crate) struct MockShared {
    /// Streams currently open: (device id, lost flag).
    open: Vec<(String, Arc<AtomicBool>)>,
}

/// The mock backend.
#[derive(Default)]
pub struct MockBackend {
    pub(crate) devices: Mutex<Vec<MockDevice>>,
    pub(crate) shared: Mutex<MockShared>,
    pub(crate) opens: AtomicU64,
    /// The granted report for non-exact devices (shared 48 kHz f32 by
    /// default).
    pub(crate) fallback: Mutex<Option<StreamReport>>,
}

impl MockBackend {
    /// Adds a device.
    pub fn add(&self, device: MockDevice) {
        self.devices.lock().unwrap().push(device);
    }

    /// Pulls the device out from under every open stream on it — the
    /// unplug. Fires `DeviceLost` on the affected streams.
    pub fn lose_device(&self, report: &StreamReport) {
        let id = report
            .device_id
            .clone()
            .unwrap_or_else(|| "default".to_owned());
        self.devices.lock().unwrap().retain(|d| d.id != id);
        let lost = self.shared.lock().unwrap();
        for (open_id, flag) in &lost.open {
            if *open_id == id {
                flag.store(true, Ordering::Release);
            }
        }
    }

    /// How many opens were attempted.
    pub fn open_count(&self) -> u64 {
        self.opens.load(Ordering::Acquire)
    }
}

impl OutputBackend for MockBackend {
    fn name(&self) -> &'static str {
        "mock"
    }

    fn open(
        &self,
        request: &StreamRequest,
        _source: Box<dyn RenderSource>,
        events: Sender<OutputEvent>,
        delay_us: Arc<AtomicU64>,
    ) -> Result<OpenedStream, OpenError> {
        self.opens.fetch_add(1, Ordering::AcqRel);
        // The requested device, else the first (a None device request
        // means "the default"). Scoped so the guard drops before any
        // re-entrant lock.
        let device = {
            let devices = self.devices.lock().unwrap();
            devices
                .iter()
                .find(|d| Some(&d.id) == request.device.as_ref())
                .cloned()
                .or_else(|| devices.first().cloned())
        };
        let Some(device) = device else {
            return Err(OpenError::Failed("no device".into()));
        };
        let report = if request.exclusive && device.exact {
            StreamReport {
                sharing: Sharing::Exclusive,
                rate: request.rate,
                channels: request.channels,
                format: request.format,
                hardware: None,
                buffer_frames: 4096,
                device_id: None,
            }
        } else {
            self.fallback
                .lock()
                .unwrap()
                .clone()
                .unwrap_or(StreamReport {
                    sharing: Sharing::Shared,
                    rate: 48_000,
                    channels: 2,
                    format: OutputFormat::F32,
                    hardware: None,
                    buffer_frames: 2048,
                    device_id: None,
                })
        };
        // The "stream": a lost flag the unplug sets, watched by a thread
        // that emits the event.
        let lost = Arc::new(AtomicBool::new(false));
        let mut report = report;
        report.device_id = Some(device.id.clone());
        self.shared
            .lock()
            .unwrap()
            .open
            .push((device.id.clone(), lost.clone()));
        let stopped = Arc::new(AtomicBool::new(false));
        let watcher_stopped = stopped.clone();
        let watcher_lost = lost.clone();
        let watcher_events = events.clone();
        let open_stopped = stopped.clone();
        let watcher = std::thread::spawn(move || {
            loop {
                if watcher_stopped.load(Ordering::Acquire) {
                    return;
                }
                if watcher_lost.load(Ordering::Acquire) {
                    let _ = watcher_events.send(OutputEvent::DeviceLost);
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
        });
        // Publish a plausible delay: the mock's buffer at its rate.
        delay_us.store(
            (report.buffer_frames as u64 * 1_000_000) / u64::from(report.rate),
            Ordering::Release,
        );
        Ok(OpenedStream {
            report,
            delay_us,
            stop: Box::new(move || {
                open_stopped.store(true, Ordering::Release);
                let _ = watcher.join();
            }),
            stopped,
        })
    }
}
