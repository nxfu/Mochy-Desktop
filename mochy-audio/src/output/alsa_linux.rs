//! The ALSA `hw:` exclusive backend (`exclusive-linux`).
//!
//! Port of `exclusive_pass.cpp`'s AAudio arm onto ALSA direct hardware
//! access (upstream pin `bde01cb4`; blueprint ch. 11 Table 23, ch. 12
//! matrix): open the `hw:` device — not `plughw`, no plug conversions —
//! demand the song's rate/channels/format, **read the parameters back
//! and compare before the first buffer** (the exactness gate: ALSA
//! silently adjusts rates it can't do exactly, which is exactly the
//! "asked for N Hz exclusive, the device gave something else" refusal),
//! then run a blocking write loop fed by the render source.
//!
//! Device loss (unplug, another process taking the card) surfaces as a
//! write error → [`OutputEvent::DeviceLost`] → the router's
//! reinit-at-position recovery.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, mpsc::Sender};

use alsa::pcm::{Access, Format, HwParams, PCM};
use alsa::{Direction, ValueOr};

use super::{
    EdgeConverter, ExactnessFailure, OpenError, OpenedStream, OutputBackend, OutputEvent,
    OutputFormat, PcmBlock, RenderSource, Sharing, StreamReport, StreamRequest,
};

/// The ALSA `hw:` backend.
#[derive(Debug, Default)]
pub struct AlsaHw;

/// The exactness gate's ALSA arm: what the params came back as versus
/// what was asked (`exclusive_pass.cpp:184-189` — ALSA's silent
/// adjustment plays the role of AAudio's downgraded grant).
fn verify_hw(request: &StreamRequest, hwp: &HwParams<'_>) -> Result<(), ExactnessFailure> {
    let actual_rate = hwp.get_rate().unwrap_or(0);
    let actual_channels = hwp.get_channels().unwrap_or(0) as usize;
    let actual_format = hwp.get_format().unwrap_or(Format::Unknown);
    let wanted = alsa_format(request.format);
    if actual_rate != request.rate || actual_channels != request.channels || actual_format != wanted
    {
        return Err(ExactnessFailure {
            sharing: Some(Sharing::Exclusive),
            rate: (actual_rate != request.rate).then_some(actual_rate),
            channels: (actual_channels != request.channels).then_some(actual_channels),
            format: (actual_format != wanted).then_some(request.format),
            hardware: None,
            hardware_note: None,
        });
    }
    Ok(())
}

/// The ALSA container for a request format. 24-bit rides a 32-bit slot
/// (S24_LE) — the same "top of the slot" packing UsbPass uses for a
/// 24-bit DAC in a 4-byte subslot.
fn alsa_format(format: OutputFormat) -> Format {
    match format {
        OutputFormat::F32 => Format::float(),
        OutputFormat::I16 => Format::s16(),
        OutputFormat::I24 => Format::s24(),
        OutputFormat::I32 => Format::s32(),
    }
}

fn container_bits(format: OutputFormat) -> u32 {
    match format {
        OutputFormat::F32 | OutputFormat::I32 => 32,
        OutputFormat::I24 => 32, // S24_LE: 24 valid bits in a 32-bit slot
        OutputFormat::I16 => 16,
    }
}

impl OutputBackend for AlsaHw {
    fn name(&self) -> &'static str {
        "alsa-hw"
    }

    fn open(
        &self,
        request: &StreamRequest,
        mut source: Box<dyn RenderSource>,
        events: Sender<OutputEvent>,
        delay_us: Arc<AtomicU64>,
    ) -> Result<OpenedStream, OpenError> {
        // `hw:` devices only — a plug layer would silently convert, the
        // opposite of exclusive.
        let name = request.device.clone().unwrap_or_else(|| "default".into());
        if name.starts_with("plug") {
            return Err(OpenError::Failed(
                "exclusive output needs a hw: device, not a plug".into(),
            ));
        }
        let pcm = PCM::new(&name, Direction::Playback, false)
            .map_err(|e| OpenError::Failed(format!("alsa open {name}: {e}")))?;
        let buffer_frames;
        {
            let hwp =
                HwParams::any(&pcm).map_err(|e| OpenError::Failed(format!("hw params: {e}")))?;
            hwp.set_access(Access::RWInterleaved)
                .map_err(|e| OpenError::Failed(e.to_string()))?;
            hwp.set_format(alsa_format(request.format))
                .map_err(|e| OpenError::Failed(e.to_string()))?;
            hwp.set_channels(request.channels as u32)
                .map_err(|e| OpenError::Failed(e.to_string()))?;
            // `ValueOr::Nearest`: ALSA picks the nearest rate it can do —
            // the read-back below refuses anything but exact.
            hwp.set_rate(request.rate, ValueOr::Nearest)
                .map_err(|e| OpenError::Failed(e.to_string()))?;
            // ~1 s buffered, like the upstream rings
            // (exclusive_pass.cpp:198).
            hwp.set_buffer_size_near(i64::from(request.rate))
                .map_err(|e| OpenError::Failed(e.to_string()))?;
            pcm.hw_params(&hwp)
                .map_err(|e| OpenError::Failed(e.to_string()))?;
            // The exactness gate, BEFORE a single buffer.
            if let Err(failure) = verify_hw(request, &hwp) {
                let _ = pcm.drop();
                return Err(OpenError::NotExact(failure));
            }
            buffer_frames = hwp.get_buffer_size().unwrap_or(i64::from(request.rate)) as usize;
        }
        pcm.prepare()
            .map_err(|e| OpenError::Failed(e.to_string()))?;
        let report = StreamReport {
            sharing: Sharing::Exclusive,
            rate: request.rate,
            channels: request.channels,
            format: request.format,
            hardware: None,
            buffer_frames,
            device_id: Some(name),
        };
        let stopped = Arc::new(AtomicBool::new(false));
        let thread_stopped = stopped.clone();
        let thread_delay = delay_us.clone();
        let rate = request.rate;
        let channels = request.channels;
        let bits = container_bits(request.format);
        let source_bits = match request.format {
            OutputFormat::I24 => 24,
            other => container_bits(other),
        };
        let format = request.format;
        let thread = std::thread::Builder::new()
            .name("mochy-alsa-exclusive".into())
            .spawn(move || {
                let pcm = pcm;
                let mut converter = EdgeConverter::new_exact(format, bits, source_bits);
                let mut scratch = vec![0f32; 8192 * channels];
                let mut block = PcmBlock::default();
                let mut i32_scratch: Vec<i32> = Vec::new();
                loop {
                    if thread_stopped.load(Ordering::Acquire) {
                        let _ = pcm.drop();
                        return;
                    }
                    // ~50 ms per write keeps the latency close without
                    // syscall churn.
                    let frames = (rate as usize / 20).clamp(64, 8192);
                    let slice = &mut scratch[..frames * channels];
                    source.render(slice, channels);
                    converter.convert(slice, &mut block);
                    // Publish the device's own delay measure.
                    if let Ok(delay) = pcm.delay()
                        && delay >= 0
                    {
                        thread_delay.store(
                            (delay as u64 * 1_000_000) / u64::from(rate),
                            Ordering::Release,
                        );
                    }
                    // Write through the typed IO for the container.
                    let ok = match format {
                        OutputFormat::F32 => block
                            .as_f32()
                            .map(|samples| {
                                write_typed(&pcm, samples, channels, &events, |pcm, buf| {
                                    pcm.io_f32().map(|io| io.writei(buf)).and_then(|r| r)
                                })
                            })
                            .unwrap_or(true),
                        OutputFormat::I16 => block
                            .as_i16()
                            .map(|samples| {
                                write_typed(&pcm, samples, channels, &events, |pcm, buf| {
                                    pcm.io_i16().map(|io| io.writei(buf)).and_then(|r| r)
                                })
                            })
                            .unwrap_or(true),
                        OutputFormat::I24 | OutputFormat::I32 => {
                            // 24-in-32 and 32-bit both land in i32 slots;
                            // the packed bytes are re-viewed as words.
                            let (_, bytes) = block.as_packed().unwrap_or((32, &[][..]));
                            i32_scratch.clear();
                            i32_scratch.extend(
                                bytes
                                    .as_chunks::<4>()
                                    .0
                                    .iter()
                                    .map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]])),
                            );
                            write_typed(&pcm, &i32_scratch, channels, &events, |pcm, buf| {
                                if format == OutputFormat::I24 {
                                    pcm.io_i32_s24().map(|io| io.writei(buf)).and_then(|r| r)
                                } else {
                                    pcm.io_i32().map(|io| io.writei(buf)).and_then(|r| r)
                                }
                            })
                        }
                    };
                    if !ok {
                        return;
                    }
                }
            })
            .map_err(|e| OpenError::Failed(e.to_string()))?;
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
}

/// One typed `writei` with xrun recovery; `false` = the device is gone
/// (the loss event has been sent).
fn write_typed<S: alsa::pcm::IoFormat>(
    pcm: &PCM,
    mut buf: &[S],
    channels: usize,
    events: &Sender<OutputEvent>,
    writei: impl Fn(&PCM, &[S]) -> alsa::Result<usize>,
) -> bool {
    while !buf.is_empty() {
        match writei(pcm, buf) {
            Ok(written) => {
                let written = written.min(buf.len() / channels.max(1));
                if written == 0 {
                    return true;
                }
                buf = &buf[written * channels.max(1)..];
            }
            Err(err) if err.errno() == -32 /* -EPIPE */ => {
                // An underrun (-EPIPE): recover and keep going.
                if pcm.prepare().is_err() {
                    let _ = events.send(OutputEvent::DeviceLost);
                    return false;
                }
            }
            Err(err) if err.errno() == -86 /* -ESTRPIPE */ => {
                if pcm.resume().is_err() {
                    let _ = events.send(OutputEvent::DeviceLost);
                    return false;
                }
            }
            Err(_) => {
                // Device loss or a fatal error.
                let _ = events.send(OutputEvent::DeviceLost);
                return false;
            }
        }
    }
    true
}
