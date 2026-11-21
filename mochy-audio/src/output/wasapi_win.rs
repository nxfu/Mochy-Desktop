//! The WASAPI exclusive backend (`exclusive-win`).
//!
//! Port of `exclusive_pass.cpp`'s gate onto WASAPI (upstream pin
//! `bde01cb4`; blueprint ch. 11 Table 23, ch. 12 matrix): open the
//! device **exclusive, event-driven, low-latency** at exactly the
//! song's rate/channels/format, then verify what was granted before the
//! first buffer — the sharing mode through the client's own report.
//! COM threading pins the whole open + verify + render loop to one
//! dedicated thread (the crate's own examples do the same); the open
//! result comes back through a channel before [`OpenedStream`] is
//! returned.
//!
//! The render loop is the event-driven pass from the upstream C++
//! (`onData`): wake on the period event, pull the available frames from
//! the render source, convert bit-exactly (no dither — exclusive means
//! untouched), write. Device death (unplug, another exclusive taker)
//! surfaces through the write path → [`OutputEvent::DeviceLost`] → the
//! router's reinit-at-position recovery.
//!
//! The measured stream latency (`IAudioClient::GetStreamLatency`) is
//! published as the callback-to-output delay — the crossmix
//! start-latency input (Phase 20).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, mpsc};

use wasapi::{DeviceEnumerator, SampleType, StreamMode, WaveFormat};

use super::{
    EdgeConverter, ExactnessFailure, OpenError, OpenedStream, OutputBackend, OutputEvent,
    OutputFormat, PcmBlock, RenderSource, Sharing, StreamReport, StreamRequest,
};

/// The WASAPI-exclusive backend.
#[derive(Debug, Default)]
pub struct WasapiExclusive;

/// Container bits for a request format. WASAPI's WAVEFORMATEXTENSIBLE
/// carries wBitsPerSample as the container size; exclusive 24-bit opens
/// a 24-bit container.
fn container_bits(format: OutputFormat) -> u32 {
    match format {
        OutputFormat::F32 | OutputFormat::I32 => 32,
        OutputFormat::I24 => 24,
        OutputFormat::I16 => 16,
    }
}

/// What the stream thread reports back to `open`.
enum OpenOutcome {
    Ready(StreamReport),
    NotExact(ExactnessFailure),
    Failed(String),
}

impl OutputBackend for WasapiExclusive {
    fn name(&self) -> &'static str {
        "wasapi-exclusive"
    }

    fn open(
        &self,
        request: &StreamRequest,
        source: Box<dyn RenderSource>,
        events: Sender<OutputEvent>,
        delay_us: Arc<AtomicU64>,
    ) -> Result<OpenedStream, OpenError> {
        // COM owns this thread for the stream's whole life.
        let (tx, rx) = mpsc::channel::<OpenOutcome>();
        let stopped = Arc::new(AtomicBool::new(false));
        let thread_stopped = stopped.clone();
        let thread_delay = delay_us.clone();
        let bits = container_bits(request.format);
        let source_bits = match request.format {
            OutputFormat::I24 => 24,
            other => container_bits(other),
        };
        let rate = request.rate;
        let channels = request.channels;
        let device_id = request.device.clone();
        let format = request.format;
        let thread = std::thread::Builder::new()
            .name("mochy-wasapi-exclusive".into())
            .spawn(move || {
                let outcome = run_stream(
                    device_id,
                    rate,
                    channels,
                    bits,
                    source_bits,
                    format,
                    source,
                    events,
                    thread_delay,
                    thread_stopped.clone(),
                );
                let _ = tx.send(outcome);
                // The thread parks until stop; the event loop continues
                // inside run_stream.
            })
            .map_err(|e| OpenError::Failed(e.to_string()))?;
        match rx
            .recv()
            .map_err(|_| OpenError::Failed("stream thread died".into()))?
        {
            OpenOutcome::Ready(report) => {
                let join_stopped = stopped.clone();
                Ok(OpenedStream {
                    report,
                    delay_us,
                    stop: Box::new(move || {
                        join_stopped.store(true, Ordering::Release);
                        let _ = thread.join();
                    }),
                    stopped,
                })
            }
            OpenOutcome::NotExact(failure) => {
                stopped.store(true, Ordering::Release);
                let _ = thread.join();
                Err(OpenError::NotExact(failure))
            }
            OpenOutcome::Failed(why) => {
                stopped.store(true, Ordering::Release);
                let _ = thread.join();
                Err(OpenError::Failed(why))
            }
        }
    }
}

/// The whole stream life, on its COM thread. Sends the open outcome,
/// then runs the event loop until stopped.
fn run_stream(
    device_id: Option<String>,
    rate: u32,
    channels: usize,
    bits: u32,
    source_bits: u32,
    format: OutputFormat,
    mut source: Box<dyn RenderSource>,
    events: Sender<OutputEvent>,
    delay_us: Arc<AtomicU64>,
    stopped: Arc<AtomicBool>,
) -> OpenOutcome {
    // initialize_mta answers an HRESULT directly.
    let com = wasapi::initialize_mta();
    if com.is_err() {
        return OpenOutcome::Failed(format!("com init failed: {com:?}"));
    }
    let enumerator = match DeviceEnumerator::new() {
        Ok(enumerator) => enumerator,
        Err(e) => return OpenOutcome::Failed(format!("enumerator: {e}")),
    };
    let device = match &device_id {
        Some(id) => match enumerator.get_device(id) {
            Ok(device) => device,
            Err(e) => return OpenOutcome::Failed(format!("device {id}: {e}")),
        },
        None => match enumerator.get_default_device(wasapi::Direction::Render) {
            Ok(device) => device,
            Err(e) => return OpenOutcome::Failed(e.to_string()),
        },
    };
    let mut client = match device.get_iaudioclient() {
        Ok(client) => client,
        Err(e) => return OpenOutcome::Failed(e.to_string()),
    };
    // The exclusive format: exactly what was asked ("exclusive streams
    // only exist on the low latency path", exclusive_pass.cpp:168-169).
    let sample_type = match format {
        OutputFormat::F32 => SampleType::Float,
        _ => SampleType::Int,
    };
    let wavefmt = WaveFormat::new(
        bits as usize,
        bits as usize,
        sample_type,
        rate as usize,
        channels,
        None,
    );
    // Supported exclusive at all? (Handles the device-in-use and
    // not-allowed quirks the way the crate's example does.)
    let wavefmt = match client.is_supported_exclusive_with_quirks(&wavefmt) {
        Ok(fmt) => fmt,
        Err(e) => {
            return OpenOutcome::NotExact(ExactnessFailure {
                sharing: None,
                rate: None,
                channels: None,
                format: Some(format),
                hardware: None,
                hardware_note: Some(e.to_string()),
            });
        }
    };
    let granted_rate = wavefmt.get_samplespersec();
    let granted_channels = wavefmt.get_nchannels() as usize;
    let granted_bits = u32::from(wavefmt.get_bitspersample());
    let granted_format = match sample_type {
        SampleType::Float => OutputFormat::F32,
        SampleType::Int => match granted_bits {
            16 => OutputFormat::I16,
            24 => OutputFormat::I24,
            _ => OutputFormat::I32,
        },
    };
    // The exactness gate: the quirks-adjusted format must still be what
    // was asked (`exclusive_pass.cpp:184-189`).
    if granted_rate != rate || granted_channels != channels || granted_format != format {
        return OpenOutcome::NotExact(ExactnessFailure {
            sharing: Some(Sharing::Exclusive),
            rate: (granted_rate != rate).then_some(granted_rate),
            channels: (granted_channels != channels).then_some(granted_channels),
            format: (granted_format != format).then_some(granted_format),
            hardware: None,
            hardware_note: None,
        });
    }
    let (_, min_period) = match client.get_device_period() {
        Ok(periods) => periods,
        Err(e) => return OpenOutcome::Failed(e.to_string()),
    };
    // ~10 ms period, aligned the way the drivers want (the crate's
    // 128-byte alignment covers Intel HDA).
    let period_hns = match client.calculate_aligned_period_near(
        10 * min_period.max(30_000),
        Some(128),
        &wavefmt,
    ) {
        Ok(period) => period,
        Err(e) => return OpenOutcome::Failed(e.to_string()),
    };
    if let Err(e) = client.initialize_client(
        &wavefmt,
        wasapi::Direction::Render,
        StreamMode::EventsExclusive { period_hns },
        None,
    ) {
        return OpenOutcome::Failed(format!("exclusive init: {e}"));
    }
    // Sharing mode granted: the client's own report.
    if client.get_sharemode() != Some(wasapi::ShareMode::Exclusive) {
        return OpenOutcome::NotExact(ExactnessFailure {
            sharing: client.get_sharemode().map(|mode| match mode {
                wasapi::ShareMode::Shared => Sharing::Shared,
                wasapi::ShareMode::Exclusive => Sharing::Exclusive,
            }),
            rate: None,
            channels: None,
            format: None,
            hardware: None,
            hardware_note: None,
        });
    }
    let buffer_frames = match client.get_buffer_size() {
        Ok(frames) => frames as usize,
        Err(e) => return OpenOutcome::Failed(e.to_string()),
    };
    let event = match client.set_get_eventhandle() {
        Ok(event) => event,
        Err(e) => return OpenOutcome::Failed(e.to_string()),
    };
    let render = match client.get_audiorenderclient() {
        Ok(render) => render,
        Err(e) => return OpenOutcome::Failed(e.to_string()),
    };
    if let Err(e) = client.start_stream() {
        return OpenOutcome::Failed(e.to_string());
    }
    let device_name = device
        .get_friendlyname()
        .unwrap_or_else(|_| "output".into());
    let report = StreamReport {
        sharing: Sharing::Exclusive,
        rate,
        channels,
        format,
        hardware: None,
        buffer_frames,
        device_id: Some(device_name),
    };
    // The event-driven pass (onData): wake per period, render what's
    // available, write it bit-exactly.
    let mut converter = EdgeConverter::new_exact(format, bits, source_bits);
    let mut scratch = vec![0f32; 8192 * channels];
    let mut block = PcmBlock::default();
    loop {
        if stopped.load(Ordering::Acquire) {
            let _ = client.stop_stream();
            return OpenOutcome::Ready(report);
        }
        if event.wait_for_event(50).is_err() {
            // Timeout: loop back around so a stop request lands.
            continue;
        }
        let available = match client.get_available_space_in_frames() {
            Ok(frames) => frames as usize,
            Err(_) => {
                let _ = events.send(OutputEvent::DeviceLost);
                return OpenOutcome::Ready(report);
            }
        };
        if available == 0 {
            continue;
        }
        let frames = available.min(8192);
        let slice = &mut scratch[..frames * channels];
        source.render(slice, channels);
        converter.convert(slice, &mut block);
        // The stream latency, in µs (100 ns units → µs / 10).
        if let Ok(latency) = client.get_stream_latency() {
            delay_us.store((latency / 10).max(0) as u64, Ordering::Release);
        }
        if render.write_to_device(&block.to_le_bytes(), None).is_err() {
            let _ = events.send(OutputEvent::DeviceLost);
            return OpenOutcome::Ready(report);
        }
    }
}
