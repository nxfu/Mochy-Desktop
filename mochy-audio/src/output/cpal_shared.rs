//! The shared cpal backend — the default output on every OS.
//!
//! The blueprint's "One cpal callback pulls from a master ring" (ch. 11)
//! with the mixer as the render source: the callback calls
//! [`RenderSource::render`] directly (lock-free by contract), converts
//! at the stream edge when the device doesn't take f32 (16-bit gets the
//! TPDF dither — the PrecisionSink arm), and publishes the measured
//! callback-to-output delay from cpal's `playback - callback`
//! timestamps.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, mpsc::Sender};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use super::{
    OpenError, OpenedStream, OutputEvent, OutputFormat, RenderSource, Sharing, StreamReport,
    StreamRequest,
};
use crate::precision::TpdfDither;

/// Opens the shared stream for a request. The device negotiates: f32 at
/// a supported config when it takes float, else 16-bit dithered — picked
/// from `supported_output_configs` up front so the render source is
/// built exactly once (a failed `build_output_stream` consumes it).
pub fn open_shared(
    request: &StreamRequest,
    mut source: Box<dyn RenderSource>,
    events: Sender<OutputEvent>,
    delay_us: Arc<AtomicU64>,
) -> Result<OpenedStream, OpenError> {
    let host = cpal::default_host();
    let device = match &request.device {
        Some(id) => host
            .devices()
            .map_err(|e| OpenError::Failed(e.to_string()))?
            .find(|d| d.to_string() == *id)
            .ok_or_else(|| OpenError::Failed(format!("no device named {id}")))?,
        None => host
            .default_output_device()
            .ok_or_else(|| OpenError::Failed("no output device".into()))?,
    };
    let device_name = device.to_string();
    // Pick the config: prefer the request's rate, prefer f32, else i16
    // (dithered). Everything else the host offers is out of scope for
    // the shared path.
    let mut chosen: Option<(cpal::StreamConfig, OutputFormat)> = None;
    for supported in device
        .supported_output_configs()
        .map_err(|e| OpenError::Failed(e.to_string()))?
    {
        let format = match supported.sample_format() {
            cpal::SampleFormat::F32 => OutputFormat::F32,
            cpal::SampleFormat::I16 => OutputFormat::I16,
            _ => continue,
        };
        // Prefer f32 over i16; prefer the request's rate over the
        // device default.
        let prefers = chosen.is_none()
            || (chosen.is_some_and(|(_, f)| f == OutputFormat::I16) && format == OutputFormat::F32);
        if !prefers {
            continue;
        }
        let rate_ok = supported.min_sample_rate() <= request.rate
            && request.rate <= supported.max_sample_rate();
        let config = if rate_ok && request.rate != 0 {
            supported.with_sample_rate(request.rate).config()
        } else {
            supported
                .with_sample_rate(supported.max_sample_rate())
                .config()
        };
        chosen = Some((config, format));
        if rate_ok && format == OutputFormat::F32 {
            break;
        }
    }
    let Some((config, format)) = chosen else {
        return Err(OpenError::Failed(
            "the device takes neither f32 nor i16 output".into(),
        ));
    };
    let channels = config.channels as usize;
    let report = StreamReport {
        sharing: Sharing::Shared,
        rate: config.sample_rate,
        channels,
        format,
        hardware: None,
        buffer_frames: 512,
        device_id: Some(device_name),
    };
    let stopped = Arc::new(AtomicBool::new(false));
    let delay = delay_us.clone();
    let stream = match format {
        OutputFormat::F32 => device
            .build_output_stream(
                config,
                move |data: &mut [f32], info: &cpal::OutputCallbackInfo| {
                    source.render(data, channels);
                    publish_delay(&delay, info);
                },
                move |error| {
                    // cpal signals device death through the error
                    // callback — the desktop unplug path.
                    if matches!(error.kind(), cpal::ErrorKind::DeviceNotAvailable) {
                        let _ = events.send(OutputEvent::DeviceLost);
                    }
                },
                None,
            )
            .map_err(|e| OpenError::Failed(e.to_string()))?,
        OutputFormat::I16 => {
            let mut dither = TpdfDither::default();
            let mut scratch = vec![0f32; 8192 * channels];
            device
                .build_output_stream(
                    config,
                    move |data: &mut [i16], info: &cpal::OutputCallbackInfo| {
                        let frames = data.len() / channels;
                        if scratch.len() < frames * channels {
                            scratch.resize(frames * channels, 0.0);
                        }
                        let slice = &mut scratch[..frames * channels];
                        source.render(slice, channels);
                        for (out, sample) in data.iter_mut().zip(slice) {
                            *out = dither.dither_to_16(*sample);
                        }
                        publish_delay(&delay, info);
                    },
                    move |error| {
                        if matches!(error.kind(), cpal::ErrorKind::DeviceNotAvailable) {
                            let _ = events.send(OutputEvent::DeviceLost);
                        }
                    },
                    None,
                )
                .map_err(|e| OpenError::Failed(e.to_string()))?
        }
        _ => unreachable!("shared streams are f32 or i16"),
    };
    finish(report, stream, stopped, delay_us)
}

fn finish(
    report: StreamReport,
    stream: cpal::Stream,
    stopped: Arc<AtomicBool>,
    delay_us: Arc<AtomicU64>,
) -> Result<OpenedStream, OpenError> {
    stream
        .play()
        .map_err(|e| OpenError::Failed(e.to_string()))?;
    stopped.store(false, Ordering::Release);
    let stream = Arc::new(stream);
    let stop_stream = stream.clone();
    Ok(OpenedStream {
        report,
        delay_us,
        stop: Box::new(move || {
            stop_stream.pause().ok();
        }),
        stopped,
    })
}

/// Publishes the measured callback-to-output delay from cpal's
/// timestamps (`playback - callback` — the predicted instant the data
/// will be delivered to the device).
fn publish_delay(delay_us: &AtomicU64, info: &cpal::OutputCallbackInfo) {
    let timestamp = info.timestamp();
    if let Some(delta) = timestamp
        .playback
        .checked_duration_since(timestamp.callback)
    {
        delay_us.store(delta.as_micros() as u64, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_the_default_device_or_reports_cleanly() {
        // On a headless CI box there may be no device; both outcomes are
        // valid, but a panic is not.
        struct Silence;
        impl RenderSource for Silence {
            fn render(&mut self, out: &mut [f32], _channels: usize) {
                out.fill(0.0);
            }
        }
        let request = StreamRequest {
            rate: 0,
            channels: 2,
            format: OutputFormat::F32,
            exclusive: false,
            device: None,
        };
        let (events, _rx) = std::sync::mpsc::channel();
        match open_shared(
            &request,
            Box::new(Silence),
            events,
            Arc::new(AtomicU64::new(0)),
        ) {
            Ok(stream) => {
                // If a device exists: a shared f32 (or dithered 16-bit)
                // stream with a sane report.
                assert_eq!(stream.report.sharing, Sharing::Shared);
                // u64 microseconds — sanity-check the magnitude (a shared
                // stream buffers at most a few hundred ms), the sign is
                // already in the type.
                assert!(stream.output_delay_us() < 60_000_000);
                stream.stop();
            }
            Err(OpenError::Failed(why)) => {
                assert!(!why.is_empty());
            }
            Err(other) => panic!("shared open must not hit the exactness gate: {other}"),
        }
    }
}
