//! The OutputRouter — shared cpal by default, exclusive per OS, verified
//! before use.
//!
//! Port of `playback/audio/pass/` routing (upstream pin `bde01cb4`):
//! `PassRouterSink` (route each song normal-or-exclusive, hand over
//! without a song starting under another, fall back mid-song with no
//! error shown) + `AudioPass.canTake/skipFor`'s decision shape +
//! `exclusive_pass.cpp`'s exactness gate, on the blueprint's desktop
//! matrix (ch. 11 Table 23 "Output routing", ch. 12):
//!
//! | OS | default | exclusive | notes |
//! |----|---------|-----------|-------|
//! | Windows | shared cpal | WASAPI-exclusive (`exclusive-win`) | verified bit-exact |
//! | Linux | shared cpal | ALSA `hw:` (`exclusive-linux`) | verified bit-exact |
//! | macOS | shared CoreAudio | **none** — hog mode is fragile; documented limitation (blueprint ch. 12) |
//!
//! The exactness gate ([`verify_exclusive`]) is the direct port of
//! `exclusive_pass.cpp:184-195`:
//!
//! ```text
//! exact = sharingMode == EXCLUSIVE && sampleRate == rate &&
//!         channelCount == channels && format == format
//! // from android 14 the hardware's own format is readable: a stream
//! // converted on its way there isn't exact
//! if (exact && hardwareSampleRate && hardwareFormat)
//!     exact = hardwareSampleRate == rate && hardwareFormat == format
//! ```
//!
//! A stream that fails the gate is **refused before a single buffer**
//! ("android didn't grant the output exactly as asked") and the song
//! plays shared; a stream that dies mid-song (device unplugged, taken
//! over) emits [`OutputEvent::DeviceLost`] and the caller re-opens and
//! resumes at position (the AudioTrack-failure recovery path ported as
//! reinit-at-position).
//!
//! The **measured callback-to-output delay** is published per stream
//! ([`OpenedStream::output_delay_us`]) — Phase 20's crossmix
//! start-latency input, directly observable on desktop.

pub mod cpal_shared;

#[cfg(all(target_os = "linux", feature = "exclusive-linux"))]
pub mod alsa_linux;
#[cfg(all(windows, feature = "exclusive-win"))]
pub mod wasapi_win;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use crate::precision::TpdfDither;

/// The sample container an output stream carries. The f32 mixer output is
/// converted at the stream edge: bit-exact packing for integer exclusive
/// containers ([`crate::pcm`]), TPDF dither for shared 16-bit
/// ([`crate::precision`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    F32,
    I16,
    I24,
    I32,
}

impl OutputFormat {
    /// Bytes per sample.
    pub fn bytes(self) -> usize {
        match self {
            OutputFormat::F32 => 4,
            OutputFormat::I16 => 2,
            OutputFormat::I24 => 3,
            OutputFormat::I32 => 4,
        }
    }
}

/// One output request. Shared mode negotiates around it (the device's
/// own defaults); exclusive mode demands it **exactly** or refuses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamRequest {
    pub rate: u32,
    pub channels: usize,
    pub format: OutputFormat,
    pub exclusive: bool,
    /// The device to open, by id; `None` = the default.
    pub device: Option<String>,
}

/// What an opened stream says about itself — the exactness gate's
/// inputs (`exclusive_pass.cpp:184-189`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamReport {
    /// The sharing mode actually granted.
    pub sharing: Sharing,
    pub rate: u32,
    pub channels: usize,
    pub format: OutputFormat,
    /// The hardware's own rate/format when the platform can read them
    /// (the Android-14 clause; WASAPI's mix format plays this role) — a
    /// stream converted on its way to the hardware isn't exact.
    pub hardware: Option<(u32, OutputFormat)>,
    /// The stream's buffered frame count.
    pub buffer_frames: usize,
    /// The device the stream runs on (the id the request named or the
    /// default's).
    pub device_id: Option<String>,
}

/// The sharing mode a stream ended up in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sharing {
    Shared,
    Exclusive,
}

/// Why an exclusive request was refused — the gate's verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExactnessFailure {
    pub sharing: Option<Sharing>,
    pub rate: Option<u32>,
    pub channels: Option<usize>,
    pub format: Option<OutputFormat>,
    pub hardware: Option<(u32, OutputFormat)>,
    /// A platform note riding the refusal (the driver's own wording).
    pub hardware_note: Option<String>,
}

impl std::fmt::Display for ExactnessFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "exclusive refused: sharing={:?} rate={:?} channels={:?} format={:?} hardware={:?}",
            self.sharing, self.rate, self.channels, self.format, self.hardware
        )
    }
}

/// The exactness gate — port of `exclusive_pass.cpp:184-195`. Every
/// field must match what was asked, including the hardware's own format
/// when the platform exposes one.
pub fn verify_exclusive(
    request: &StreamRequest,
    report: &StreamReport,
) -> Result<(), ExactnessFailure> {
    let fail = |sharing, rate, channels, format, hardware| {
        Err(ExactnessFailure {
            sharing,
            rate,
            channels,
            format,
            hardware,
            hardware_note: None,
        })
    };
    if report.sharing != Sharing::Exclusive {
        return fail(Some(report.sharing), None, None, None, None);
    }
    if report.rate != request.rate {
        return fail(None, Some(report.rate), None, None, None);
    }
    if report.channels != request.channels {
        return fail(None, None, Some(report.channels), None, None);
    }
    if report.format != request.format {
        return fail(None, None, None, Some(report.format), None);
    }
    if let Some((hw_rate, hw_format)) = report.hardware
        && (hw_rate != request.rate || hw_format != request.format)
    {
        return fail(None, None, None, None, Some((hw_rate, hw_format)));
    }
    Ok(())
}

/// What the render side of the player provides — implemented by
/// `mochy_player::Mixer`. The backends call [`RenderSource::render`] on
/// their own thread (the cpal callback, the WASAPI event thread, the
/// ALSA poll loop, the USB worker) — it must be lock-free.
pub trait RenderSource: Send {
    /// Renders `frames` interleaved frames into `out` (len =
    /// frames × channels).
    fn render(&mut self, out: &mut [f32], channels: usize);
}

/// One opened output stream.
pub struct OpenedStream {
    /// What the stream actually granted (run it through
    /// [`verify_exclusive`] for the gate).
    pub report: StreamReport,
    delay_us: Arc<AtomicU64>,
    stop: Box<dyn FnOnce() + Send>,
    stopped: Arc<std::sync::atomic::AtomicBool>,
}

impl OpenedStream {
    /// Assembles a stream from a backend's parts. `pub(crate)`: the
    /// in-tree backends live in child modules (private-field access),
    /// but the USB driver sits in `crate::usb` and needs this door —
    /// gated to exactly the configs that driver compiles in.
    #[cfg(all(feature = "usb-uac", any(windows, target_os = "linux")))]
    pub(crate) fn from_parts(
        report: StreamReport,
        delay_us: Arc<AtomicU64>,
        stop: Box<dyn FnOnce() + Send>,
        stopped: Arc<std::sync::atomic::AtomicBool>,
    ) -> OpenedStream {
        OpenedStream {
            report,
            delay_us,
            stop,
            stopped,
        }
    }

    /// The measured callback-to-output delay, in microseconds — cpal's
    /// `playback - callback` timestamp delta, WASAPI's stream latency,
    /// ALSA's pcm delay, or the USB feedback pace. Phase 20 consumes
    /// this as the crossmix start-latency input.
    pub fn output_delay_us(&self) -> u64 {
        self.delay_us.load(Ordering::Acquire)
    }

    /// Whether the stream is still running.
    pub fn is_running(&self) -> bool {
        !self.stopped.load(Ordering::Acquire)
    }

    /// Closes the stream.
    pub fn stop(self) {
        self.stopped.store(true, Ordering::Release);
        (self.stop)();
    }
}

/// Why an open failed.
#[derive(thiserror::Error, Debug)]
pub enum OpenError {
    /// The device refused the request outright (absent, busy, no such
    /// format even shared).
    #[error("open failed: {0}")]
    Failed(String),
    /// Exclusive was asked and the exactness gate refused — the report
    /// shows what the device offered instead
    /// ("android didn't grant the output exactly as asked",
    /// exclusive_pass.cpp:190-196).
    #[error("not exact: {0}")]
    NotExact(ExactnessFailure),
}

/// A pluggable backend — cpal by default, the OS-exclusive drivers
/// behind their features, [`mock`] in tests.
pub trait OutputBackend: Send + Sync {
    /// The backend's name (diagnostics).
    fn name(&self) -> &'static str;
    /// Opens a stream. `events` receives device lifecycle events.
    fn open(
        &self,
        request: &StreamRequest,
        source: Box<dyn RenderSource>,
        events: Sender<OutputEvent>,
        delay_us: Arc<AtomicU64>,
    ) -> Result<OpenedStream, OpenError>;
}

/// Device lifecycle events (the desktop `AudioDeviceCallback`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputEvent {
    /// The stream's device went away mid-song — re-open and resume at
    /// position.
    DeviceLost,
    /// A device appeared.
    DeviceAdded(String),
}

/// The exclusive-failure memory: "this song couldn't open, so the same
/// format isn't tried again on this dac until something changes"
/// (`AudioPass.failed`, the `failed` set keyed by
/// mode/device/rate/channels/bits).
#[derive(Debug, Default)]
struct FailedFormats(std::collections::HashSet<String>);

fn failure_key(request: &StreamRequest) -> String {
    format!(
        "{:?}/{}/{}/{:?}/{}",
        request.device, request.rate, request.channels, request.format, request.exclusive
    )
}

/// The router: one open stream at a time, kept across same-format songs,
/// re-opened on change or device loss, falling back to shared when
/// exclusive refuses (PassRouterSink's route + fallBack).
pub struct OutputRouter {
    backend: Arc<dyn OutputBackend>,
    current: Mutex<Option<OpenedStream>>,
    failed: Mutex<FailedFormats>,
    events: Sender<OutputEvent>,
    event_rx: Receiver<OutputEvent>,
    /// The generation bump of "the answer to may-this-song-go-exclusive
    /// could change" (`AudioPass.generation`).
    generation: std::sync::atomic::AtomicI64,
}

impl OutputRouter {
    /// A router over one backend.
    pub fn new(backend: Arc<dyn OutputBackend>) -> OutputRouter {
        let (events, event_rx) = channel();
        OutputRouter {
            backend,
            current: Mutex::new(None),
            failed: Mutex::new(FailedFormats::default()),
            events,
            event_rx,
            generation: std::sync::atomic::AtomicI64::new(0),
        }
    }

    /// The default router: the shared cpal backend (plus this platform's
    /// exclusive backends when their features are on).
    pub fn shared() -> OutputRouter {
        OutputRouter::new(Arc::new(BackendSet::default()))
    }

    /// The event feed (device loss, arrivals).
    pub fn events(&self) -> &Receiver<OutputEvent> {
        &self.event_rx
    }

    /// The routing generation — bumps whenever the may-go-exclusive
    /// answer could change (device set changed, a failure was noted).
    pub fn generation(&self) -> i64 {
        self.generation.load(Ordering::Acquire)
    }

    /// Opens or keeps the stream for a request
    /// (`PassSink.configure`: "the next song in the same format keeps
    /// the output open, so an album plays gapless").
    ///
    /// * **shared**: the request negotiates with the device (f32 first,
    ///   16-bit with dither as fallback);
    /// * **exclusive**: the exactness gate runs before the stream is
    ///   accepted; a refusal marks the format failed for this device
    ///   (until [`clear_failures`]) and **falls back to shared with no
    ///   error surfaced** (`PassRouterSink.fallBack`).
    pub fn ensure(
        &self,
        request: &StreamRequest,
        make_source: impl Fn() -> Box<dyn RenderSource>,
    ) -> Result<StreamReport, OpenError> {
        let mut current = self.current.lock().expect("router stream lock");
        // Same format on an open stream: keep it.
        if let Some(open) = current.as_ref() {
            let matches = open.is_running()
                && (!request.exclusive || open.report.sharing == Sharing::Exclusive)
                && open.report.rate == request.rate
                && open.report.channels == request.channels
                && open.report.format == request.format;
            if matches {
                return Ok(open.report.clone());
            }
            // A different format: the old stream drains what it holds
            // before the new one opens (the handover) — for the port the
            // close is immediate; the mixer's voices carry the overlap.
            let old = current.take();
            if let Some(old) = old {
                old.stop();
            }
        }
        let key = failure_key(request);
        let exclusive_blocked = request.exclusive
            && self
                .failed
                .lock()
                .expect("failure memory lock")
                .0
                .contains(&key);
        let try_exclusive = request.exclusive && !exclusive_blocked;
        if try_exclusive {
            // `PassRouterSink.fallBack`: "the exclusive output failed:
            // the song goes on the normal way from this buffer, with no
            // error shown" — a refusal (or an inexact grant) marks the
            // format failed and lands in the shared fallback below.
            let refused = match self.backend.open(
                request,
                make_source(),
                self.events.clone(),
                Arc::new(AtomicU64::new(0)),
            ) {
                Ok(stream) => match verify_exclusive(request, &stream.report) {
                    Ok(()) => {
                        let report = stream.report.clone();
                        *current = Some(stream);
                        return Ok(report);
                    }
                    Err(failure) => {
                        stream.stop();
                        Some(failure)
                    }
                },
                Err(OpenError::NotExact(failure)) => Some(failure),
                Err(OpenError::Failed(why)) => {
                    // The exclusive device wouldn't open at all
                    // (Skip.NoDac's shape) — also a refusal.
                    let _ = why;
                    None
                }
            };
            self.failed
                .lock()
                .expect("failure memory lock")
                .0
                .insert(key);
            self.generation.fetch_add(1, Ordering::AcqRel);
            let _ = refused;
        }
        // Shared (the default, or the fallback).
        let shared_request = StreamRequest {
            exclusive: false,
            ..request.clone()
        };
        let stream = self.backend.open(
            &shared_request,
            make_source(),
            self.events.clone(),
            Arc::new(AtomicU64::new(0)),
        )?;
        let report = stream.report.clone();
        *current = Some(stream);
        Ok(report)
    }

    /// Device lost mid-song: close and let the caller re-open (then
    /// `PlayerCore::resume_at_position`). Returns whether there was a
    /// stream to lose.
    pub fn handle_device_lost(&self) -> bool {
        let mut current = self.current.lock().expect("router stream lock");
        let old = current.take();
        let had = old.is_some();
        if let Some(old) = old {
            old.stop();
        }
        // A device change re-opens the exclusive question
        // (`AudioPass.update` clears `failed` on mode change).
        self.failed.lock().expect("failure memory lock").0.clear();
        self.generation.fetch_add(1, Ordering::AcqRel);
        had
    }

    /// Forgets the exclusive failures ("something changed" — the settings
    /// page's retry).
    pub fn clear_failures(&self) {
        self.failed.lock().expect("failure memory lock").0.clear();
        self.generation.fetch_add(1, Ordering::AcqRel);
    }

    /// The measured callback-to-output delay of the open stream, if any.
    pub fn output_delay_us(&self) -> Option<u64> {
        self.current
            .lock()
            .expect("router stream lock")
            .as_ref()
            .map(|stream| stream.output_delay_us())
    }
}

/// The platform backend set: shared cpal always; the exclusive backends
/// behind their cfg+feature gates. macOS documents the gap (blueprint
/// ch. 12: hog mode is fragile — shared-only, a recorded limitation).
#[derive(Debug, Default)]
pub struct BackendSet {
    #[cfg(all(windows, feature = "exclusive-win"))]
    wasapi: wasapi_win::WasapiExclusive,
    #[cfg(all(target_os = "linux", feature = "exclusive-linux"))]
    alsa: alsa_linux::AlsaHw,
}

impl OutputBackend for BackendSet {
    fn name(&self) -> &'static str {
        "platform"
    }

    fn open(
        &self,
        request: &StreamRequest,
        source: Box<dyn RenderSource>,
        events: Sender<OutputEvent>,
        delay_us: Arc<AtomicU64>,
    ) -> Result<OpenedStream, OpenError> {
        if request.exclusive {
            #[cfg(all(windows, feature = "exclusive-win"))]
            {
                return self.wasapi.open(request, source, events, delay_us);
            }
            #[cfg(all(target_os = "linux", feature = "exclusive-linux"))]
            {
                return self.alsa.open(request, source, events, delay_us);
            }
            #[allow(unreachable_code)]
            {
                let _ = (request, source, events, delay_us);
                return Err(OpenError::Failed(
                    "exclusive output is not available on this platform".into(),
                ));
            }
        }
        cpal_shared::open_shared(request, source, events, delay_us)
    }
}

/// The f32→container conversion at the stream edge.
///
/// * `F32` containers pass through untouched (the port's float default);
/// * integer containers on the **exclusive** path pack bit-exactly
///   ([`crate::pcm`] — PassPcm: "only the packing changes, never a
///   sample's value"), with 24-bit songs riding the top of wider slots
///   when the backend's container is wider (ALSA S24, USB subslots);
/// * the **shared** 16-bit path dithers (the PrecisionSink arm,
///   [`crate::precision::TpdfDither`]).
#[derive(Debug)]
pub struct EdgeConverter {
    format: OutputFormat,
    /// The container's bits per sample (16, 24 or 32) — may be wider
    /// than the song's bits for slot-packed formats.
    container_bits: u32,
    /// The song's real depth — drives the whole-step exactness.
    source_bits: u32,
    /// Bit-exact packing (exclusive) vs dither (shared 16-bit).
    exact: bool,
    dither: TpdfDither,
    scratch: Vec<u8>,
}

impl EdgeConverter {
    /// A shared-path converter (dithers 16-bit containers).
    pub fn new_shared(
        format: OutputFormat,
        container_bits: u32,
        source_bits: u32,
    ) -> EdgeConverter {
        EdgeConverter {
            format,
            container_bits,
            source_bits,
            exact: false,
            dither: TpdfDither::default(),
            scratch: Vec::new(),
        }
    }

    /// An exclusive-path converter: bit-exact packing, no dither.
    pub fn new_exact(format: OutputFormat, container_bits: u32, source_bits: u32) -> EdgeConverter {
        EdgeConverter {
            format,
            container_bits,
            source_bits,
            exact: true,
            dither: TpdfDither::default(),
            scratch: Vec::new(),
        }
    }

    /// Converts interleaved f32 into the container.
    pub fn convert(&mut self, input: &[f32], out: &mut PcmBlock) {
        out.clear();
        match self.format {
            OutputFormat::F32 => {
                // Float passthrough, untouched.
                *out.f32_mut() = input.to_vec();
            }
            OutputFormat::I16 if !self.exact => {
                // The shared 16-bit arm: TPDF dither.
                let words = out.i16_mut();
                words.clear();
                words.reserve(input.len());
                for sample in input {
                    words.push(self.dither.dither_to_16(*sample));
                }
            }
            format => {
                // Integer packing — exact by construction here (the
                // shared non-16 path has no dither arm upstream either).
                let _ = format;
                let container = self.container_bits.clamp(16, 32);
                let source = self.source_bits.min(container);
                self.scratch.clear();
                self.scratch.reserve(input.len() * 4);
                for sample in input {
                    self.scratch.extend_from_slice(&sample.to_le_bytes());
                }
                let needed = input.len() * (container as usize / 8);
                out.bytes_mut().resize(needed, 0);
                let outcome = crate::pcm::convert(
                    &self.scratch,
                    crate::precision::PcmEncoding::Float,
                    1,
                    source,
                    container,
                    out.bytes_mut(),
                    false,
                );
                let _ = outcome;
                out.mark_packed(container);
            }
        }
    }
}

/// One converted block of output audio, in whichever container the
/// stream carries.
#[derive(Debug, Default)]
pub struct PcmBlock {
    kind: BlockKind,
    f32s: Vec<f32>,
    i16s: Vec<i16>,
    bytes: Vec<u8>,
}

#[derive(Debug, Default, PartialEq, Eq, Clone, Copy)]
enum BlockKind {
    #[default]
    Empty,
    F32,
    I16,
    /// Packed integer bytes at `bits` per sample.
    Packed(u32),
}

impl PcmBlock {
    fn clear(&mut self) {
        self.kind = BlockKind::Empty;
        self.f32s.clear();
        self.i16s.clear();
        self.bytes.clear();
    }

    fn bytes_mut(&mut self) -> &mut Vec<u8> {
        &mut self.bytes
    }

    fn mark_packed(&mut self, bits: u32) {
        self.kind = BlockKind::Packed(bits);
    }

    fn f32_mut(&mut self) -> &mut Vec<f32> {
        self.kind = BlockKind::F32;
        &mut self.f32s
    }

    fn i16_mut(&mut self) -> &mut Vec<i16> {
        self.kind = BlockKind::I16;
        &mut self.i16s
    }

    /// The f32 view when the container is float.
    pub fn as_f32(&self) -> Option<&[f32]> {
        match self.kind {
            BlockKind::F32 => Some(&self.f32s),
            _ => None,
        }
    }

    /// The i16 view when the container is 16-bit dithered.
    pub fn as_i16(&self) -> Option<&[i16]> {
        match self.kind {
            BlockKind::I16 => Some(&self.i16s),
            _ => None,
        }
    }

    /// The packed-byte view with its bits-per-sample.
    pub fn as_packed(&self) -> Option<(u32, &[u8])> {
        match self.kind {
            BlockKind::Packed(bits) => Some((bits, &self.bytes)),
            _ => None,
        }
    }

    /// The frame count at a channel count.
    pub fn frames(&self, channels: usize) -> usize {
        let channels = channels.max(1);
        match self.kind {
            BlockKind::Empty => 0,
            BlockKind::F32 => self.f32s.len() / channels,
            BlockKind::I16 => self.i16s.len() / channels,
            BlockKind::Packed(bits) => self.bytes.len() / (channels * bits as usize / 8),
        }
    }

    /// Little-endian byte serialization (WASAPI's write_to_device).
    pub fn to_le_bytes(&self) -> Vec<u8> {
        match self.kind {
            BlockKind::F32 => self.f32s.iter().flat_map(|s| s.to_le_bytes()).collect(),
            BlockKind::I16 => self.i16s.iter().flat_map(|s| s.to_le_bytes()).collect(),
            BlockKind::Packed(_) => self.bytes.clone(),
            BlockKind::Empty => Vec::new(),
        }
    }
}

/// A test double: opens streams with configurable granted reports and
/// injectable device loss. Lives here (not under cfg(test)) so
/// mochy-player's hotplug tests can drive it too.
pub mod mock;

#[cfg(test)]
mod tests {
    use super::*;
    use mock::{MockBackend, MockDevice};
    use std::sync::mpsc::TryRecvError;
    use std::time::Duration;

    fn shared_request(rate: u32) -> StreamRequest {
        StreamRequest {
            rate,
            channels: 2,
            format: OutputFormat::F32,
            exclusive: false,
            device: None,
        }
    }

    struct Silence;

    impl RenderSource for Silence {
        fn render(&mut self, out: &mut [f32], _channels: usize) {
            out.fill(0.0);
        }
    }

    #[test]
    fn exactness_gate_accepts_exact_reports() {
        let request = StreamRequest {
            rate: 44_100,
            channels: 2,
            format: OutputFormat::I24,
            exclusive: true,
            device: Some("dac".into()),
        };
        let report = StreamReport {
            sharing: Sharing::Exclusive,
            rate: 44_100,
            channels: 2,
            format: OutputFormat::I24,
            hardware: None,
            buffer_frames: 4096,
            device_id: Some("dac".into()),
        };
        assert!(verify_exclusive(&request, &report).is_ok());
        // With the hardware clause: matching hardware stays exact.
        let report_hw = StreamReport {
            hardware: Some((44_100, OutputFormat::I24)),
            ..report
        };
        assert!(verify_exclusive(&request, &report_hw).is_ok());
    }

    #[test]
    fn exactness_gate_refuses_every_drift() {
        let request = StreamRequest {
            rate: 48_000,
            channels: 2,
            format: OutputFormat::I16,
            exclusive: true,
            device: None,
        };
        // Sharing mode downgraded.
        let shared = StreamReport {
            sharing: Sharing::Shared,
            rate: 48_000,
            channels: 2,
            format: OutputFormat::I16,
            hardware: None,
            buffer_frames: 1024,
            device_id: None,
        };
        assert_eq!(
            verify_exclusive(&request, &shared),
            Err(ExactnessFailure {
                sharing: Some(Sharing::Shared),
                rate: None,
                channels: None,
                format: None,
                hardware: None,
                hardware_note: None,
            })
        );
        // Rate adjusted.
        let resampled = StreamReport {
            rate: 44_100,
            ..shared.clone()
        };
        let _ = resampled;
        let resampled = StreamReport {
            sharing: Sharing::Exclusive,
            rate: 44_100,
            channels: 2,
            format: OutputFormat::I16,
            hardware: None,
            buffer_frames: 1024,
            device_id: None,
        };
        assert_eq!(
            verify_exclusive(&request, &resampled).unwrap_err().rate,
            Some(44_100)
        );
        // Channels changed.
        let mono = StreamReport {
            sharing: Sharing::Exclusive,
            rate: 48_000,
            channels: 1,
            format: OutputFormat::I16,
            hardware: None,
            buffer_frames: 1024,
            device_id: None,
        };
        assert_eq!(
            verify_exclusive(&request, &mono).unwrap_err().channels,
            Some(1)
        );
        // Format converted (float instead of the asked int).
        let floated = StreamReport {
            sharing: Sharing::Exclusive,
            rate: 48_000,
            channels: 2,
            format: OutputFormat::F32,
            hardware: None,
            buffer_frames: 1024,
            device_id: None,
        };
        assert_eq!(
            verify_exclusive(&request, &floated).unwrap_err().format,
            Some(OutputFormat::F32)
        );
        // Hardware converts on the way out (the android-14 clause).
        let converted = StreamReport {
            sharing: Sharing::Exclusive,
            rate: 48_000,
            channels: 2,
            format: OutputFormat::I16,
            hardware: Some((48_000, OutputFormat::F32)),
            buffer_frames: 1024,
            device_id: None,
        };
        assert_eq!(
            verify_exclusive(&request, &converted).unwrap_err().hardware,
            Some((48_000, OutputFormat::F32))
        );
    }

    #[test]
    fn router_keeps_same_format_stream_open() {
        // PassSink.configure: the next song in the same format keeps the
        // output open.
        let backend = Arc::new(MockBackend::default());
        backend.devices.lock().unwrap().push(MockDevice {
            id: "default".into(),
            exact: true,
        });
        let router = OutputRouter::new(backend.clone());
        let first = router
            .ensure(&shared_request(48_000), || Box::new(Silence))
            .unwrap();
        let second = router
            .ensure(&shared_request(48_000), || Box::new(Silence))
            .unwrap();
        assert_eq!(first, second);
        assert_eq!(
            backend.opens.load(Ordering::Acquire),
            1,
            "reopened needlessly"
        );
    }

    #[test]
    fn router_reopens_on_format_change() {
        let backend = Arc::new(MockBackend::default());
        backend.devices.lock().unwrap().push(MockDevice {
            id: "default".into(),
            exact: true,
        });
        let router = OutputRouter::new(backend.clone());
        router
            .ensure(&shared_request(48_000), || Box::new(Silence))
            .unwrap();
        router
            .ensure(&shared_request(44_100), || Box::new(Silence))
            .unwrap();
        assert_eq!(backend.opens.load(Ordering::Acquire), 2);
    }

    #[test]
    fn exclusive_refusal_falls_back_to_shared_and_is_remembered() {
        let backend = Arc::new(MockBackend::default());
        backend.devices.lock().unwrap().push(MockDevice {
            id: "dac".into(),
            exact: false, // grants something else
        });
        let router = OutputRouter::new(backend.clone());
        let request = StreamRequest {
            rate: 44_100,
            channels: 2,
            format: OutputFormat::I24,
            exclusive: true,
            device: Some("dac".into()),
        };
        let report = router.ensure(&request, || Box::new(Silence)).unwrap();
        // Fell back to shared (the song plays the normal way).
        assert_eq!(report.sharing, Sharing::Shared);
        // The format is marked failed: the same ask goes straight to
        // shared — one shared reopen, no exclusive attempt.
        let opens_before = backend.opens.load(Ordering::Acquire);
        let report2 = router.ensure(&request, || Box::new(Silence)).unwrap();
        assert_eq!(report2.sharing, Sharing::Shared);
        assert_eq!(
            backend.opens.load(Ordering::Acquire),
            opens_before + 1,
            "the failed exclusive format must not be retried (one shared reopen only)"
        );
        // Something changed: the failures are forgotten — the exclusive
        // attempt happens again (and is refused again by this device).
        router.clear_failures();
        router.ensure(&request, || Box::new(Silence)).unwrap();
        assert!(
            backend.opens.load(Ordering::Acquire) >= opens_before + 3,
            "clearing failures must re-try exclusive ({} opens)",
            backend.opens.load(Ordering::Acquire)
        );
    }

    #[test]
    fn device_loss_closes_and_signals() {
        let backend = Arc::new(MockBackend::default());
        let router = OutputRouter::new(backend.clone());
        backend.devices.lock().unwrap().push(MockDevice {
            id: "default".into(),
            exact: true,
        });
        let report = router
            .ensure(&shared_request(48_000), || Box::new(Silence))
            .unwrap();
        assert!(router.output_delay_us().is_some());
        // The device goes away mid-song.
        backend.lose_device(&report);
        let event = router.events().recv_timeout(Duration::from_secs(2));
        assert_eq!(event, Ok(OutputEvent::DeviceLost));
        assert!(router.handle_device_lost(), "had a stream to lose");
        assert!(router.output_delay_us().is_none());
        // The event channel is drained.
        assert_eq!(
            router.events().try_recv(),
            Err(TryRecvError::Empty),
            "loss is reported once"
        );
    }

    #[test]
    fn edge_converter_f32_passthrough() {
        let mut converter = EdgeConverter::new_shared(OutputFormat::F32, 32, 32);
        let mut block = PcmBlock::default();
        converter.convert(&[0.5, -0.5], &mut block);
        let samples = block.as_f32().unwrap();
        assert_eq!(samples, &[0.5, -0.5]);
        let mut expect = 0.5f32.to_le_bytes().to_vec();
        expect.extend_from_slice(&(-0.5f32).to_le_bytes());
        assert_eq!(block.to_le_bytes(), expect);
    }

    #[test]
    fn edge_converter_dithers_16_bit_shared() {
        let mut converter = EdgeConverter::new_shared(OutputFormat::I16, 16, 16);
        let mut block = PcmBlock::default();
        converter.convert(&[0.0; 100], &mut block);
        let words = block.as_i16().unwrap();
        assert_eq!(words.len(), 100);
        // Silence dithers to the 1-LSB floor.
        assert!(words.iter().all(|w| w.abs() <= 1));
    }

    #[test]
    fn edge_converter_packs_24_exact_in_32_slot() {
        // A whole 24-bit step in a 32-bit slot: at the top of the word
        // (the ALSA S24 / USB subslot-4 shape).
        let mut converter = EdgeConverter::new_exact(OutputFormat::I24, 32, 24);
        let mut block = PcmBlock::default();
        converter.convert(&[8_388_607.0 / 8_388_608.0], &mut block);
        let (bits, bytes) = block.as_packed().unwrap();
        assert_eq!(bits, 32);
        assert_eq!(bytes, vec![0x00, 0xFF, 0xFF, 0x7F]);
    }

    #[test]
    fn edge_converter_packs_24_exact_in_24_container() {
        // The WASAPI shape: a real 3-byte container.
        let mut converter = EdgeConverter::new_exact(OutputFormat::I24, 24, 24);
        let mut block = PcmBlock::default();
        converter.convert(&[8_388_607.0 / 8_388_608.0], &mut block);
        let (bits, bytes) = block.as_packed().unwrap();
        assert_eq!(bits, 24);
        assert_eq!(bytes, vec![0xFF, 0xFF, 0x7F]);
    }

    #[test]
    fn edge_converter_exact_16_has_no_dither() {
        // An exact half step stays the nearest whole step, deterministically.
        let mut converter = EdgeConverter::new_exact(OutputFormat::I16, 16, 16);
        let mut block = PcmBlock::default();
        converter.convert(&[0.5; 10], &mut block);
        let (_, bytes) = block.as_packed().unwrap();
        let words: Vec<i16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect();
        assert!(words.iter().all(|w| *w == 16_384), "words: {words:?}");
    }
}
