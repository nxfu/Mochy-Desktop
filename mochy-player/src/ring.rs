//! The per-voice lock-free ring buffer.
//!
//! The blueprint's buffering contract (ch. 11): "the callback owns a
//! one-second ring per voice; the decode task keeps it filled with
//! hysteresis" — single producer (the voice's decode task), single
//! consumer (the audio callback), **no locks anywhere on the reader's
//! path**. That is the desktop shape of what `exclusive_pass.cpp`
//! guarded with `try_lock` + a mutex ("the audio thread never waits: if
//! kotlin holds the queue this instant, this one buffer is silence",
//! onData at lines 113-133) and `usb_pass.cpp` with its own mutex ring:
//! here the ring itself is the only synchronization.
//!
//! Capacity is ~1 s of interleaved f32 frames at the output rate — the
//! same sizing as upstream's `max(rate, 4096)` frames
//! (exclusive_pass.cpp:198, usb_pass.cpp:393: "a second of audio waits
//! here, so the player can sleep through a slow background wake-up").
//!
//! Seeks reset the ring through a generation counter: the (now sole)
//! writer bumps the generation before zeroing the positions, and the
//! reader treats any generation change mid-block as "drop what this
//! block took" — the upstream `flush` semantics ("drops what's queued
//! and starts the frame count over", PassTransport.kt:20).

use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

#[cfg(target_pointer_width = "32")]
compile_error!("the voice ring requires 64-bit atomics");

/// Cache-line padding so head and tail never share a line.
#[repr(align(64))]
struct Padded<T>(T);

impl<T> std::ops::Deref for Padded<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> std::ops::DerefMut for Padded<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for Padded<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// A lock-free SPSC ring of interleaved f32 frames.
///
/// - **Writer**: the voice's decode task ([`RingWriter`]).
/// - **Reader**: the audio callback ([`RingReader`]).
/// - Frames are interleaved at the ring's fixed channel count.
/// - Capacity is rounded up to a power of two.
#[derive(Debug)]
struct Inner {
    /// The samples as raw f32 bits in atomic slots — atomics are
    /// race-free by construction, so the SPSC contract needs no
    /// `unsafe`; the Release/Acquire pairing on head/tail publishes the
    /// payload, the payload stores/loads themselves are Relaxed.
    buffer: Box<[AtomicU32]>,
    channels: usize,
    mask: usize,
    /// Read position (frames), owned by the reader.
    head: Padded<AtomicUsize>,
    /// Write position (frames), owned by the writer.
    tail: Padded<AtomicUsize>,
    /// Bumped by the writer on a seek reset; the reader resyncs on it.
    generation: AtomicU64,
    /// Underrun blocks seen by the reader (diagnostics; the upstream
    /// policy itself is "silence, keep going").
    underruns: AtomicU64,
}

impl Inner {
    fn capacity_frames(&self) -> usize {
        self.mask + 1
    }

    fn filled(&self) -> usize {
        let tail = self.tail.load(Ordering::Acquire);
        let head = self.head.load(Ordering::Acquire);
        tail.wrapping_sub(head)
    }
}

/// Creates a ring pair sized for ~1 second of `rate` Hz `channels` audio
/// (at least 4096 frames, mirroring upstream's `max(rate, 4096)`), plus
/// the writer and reader handles.
pub fn voice_ring(rate: u32, channels: usize) -> (RingWriter, RingReader) {
    let frames = (rate as usize).max(4096).next_power_of_two();
    voice_ring_frames(frames, channels)
}

/// Creates a ring pair with an explicit frame capacity (rounded up to a
/// power of two).
pub fn voice_ring_frames(frames: usize, channels: usize) -> (RingWriter, RingReader) {
    assert!(channels > 0, "a ring needs at least one channel");
    let frames = frames.max(2).next_power_of_two();
    let inner = Arc::new(Inner {
        buffer: (0..frames * channels)
            .map(|_| AtomicU32::new(0))
            .collect::<Vec<_>>()
            .into_boxed_slice(),
        channels,
        mask: frames - 1,
        head: Padded(AtomicUsize::new(0)),
        tail: Padded(AtomicUsize::new(0)),
        generation: AtomicU64::new(0),
        underruns: AtomicU64::new(0),
    });
    (
        RingWriter {
            inner: inner.clone(),
        },
        RingReader {
            inner,
            seen_generation: 0,
        },
    )
}

/// The decode task's half. Every method takes `&self` so one active
/// writer can be held by `Arc` on the control side and moved into the
/// slot's decode task — only one task is ever attached at a time, which
/// is the SPSC contract.
#[derive(Debug)]
pub struct RingWriter {
    inner: Arc<Inner>,
}

impl RingWriter {
    /// Free frames available to write.
    pub fn free(&self) -> usize {
        self.inner.capacity_frames() - self.inner.filled()
    }

    /// Filled frames awaiting the callback.
    pub fn filled(&self) -> usize {
        self.inner.filled()
    }

    /// Total frame capacity.
    pub fn capacity(&self) -> usize {
        self.inner.capacity_frames()
    }

    /// The channel count the ring interleaves.
    pub fn channels(&self) -> usize {
        self.inner.channels
    }

    /// Writes interleaved frames; returns how many fit.
    pub fn push(&self, frames: &[f32]) -> usize {
        let channels = self.inner.channels;
        let available = frames.len() / channels;
        let take = available.min(self.free());
        if take == 0 {
            return 0;
        }
        let tail = self.inner.tail.load(Ordering::Relaxed);
        let start = (tail & self.inner.mask) * channels;
        let copy = take * channels;
        let buffer_len = self.inner.buffer.len();
        for (at, sample) in frames[..copy].iter().enumerate() {
            let slot = (start + at) % buffer_len;
            self.inner.buffer[slot].store(sample.to_bits(), Ordering::Relaxed);
        }
        self.inner
            .tail
            .store(tail.wrapping_add(take), Ordering::Release);
        take
    }

    /// Seek reset: drops the queued audio and starts the frame count
    /// over (the upstream `flush`). Only valid when the reader is parked
    /// between blocks or tolerant of a dropped block — the generation
    /// counter makes the reader discard any block it was mid-way
    /// through.
    pub fn reset(&self) {
        self.inner.generation.fetch_add(1, Ordering::SeqCst);
        self.inner.tail.store(0, Ordering::SeqCst);
        self.inner.head.store(0, Ordering::SeqCst);
    }
}

/// The audio callback's half. One reader at a time; the block protocol
/// (read [`generation`], pop, re-check) keeps a concurrent reset from
/// tearing a block.
#[derive(Debug)]
pub struct RingReader {
    inner: Arc<Inner>,
    seen_generation: u64,
}

impl RingReader {
    /// Reads up to `out.len() / channels` frames into `out`; returns the
    /// frames actually taken. Missing frames are the caller's silence —
    /// the underrun policy ported from exclusive_pass.cpp's onData.
    pub fn pop(&mut self, out: &mut [f32]) -> usize {
        let channels = self.inner.channels;
        let want = out.len() / channels;
        let filled = self.inner.filled();
        let take = want.min(filled);
        if take < want {
            self.inner.underruns.fetch_add(1, Ordering::Relaxed);
        }
        if take == 0 {
            return 0;
        }
        let head = self.inner.head.load(Ordering::Relaxed);
        let start = (head & self.inner.mask) * channels;
        let copy = take * channels;
        let buffer_len = self.inner.buffer.len();
        for (at, value) in out[..copy].iter_mut().enumerate() {
            let slot = (start + at) % buffer_len;
            *value = f32::from_bits(self.inner.buffer[slot].load(Ordering::Relaxed));
        }
        self.inner
            .head
            .store(head.wrapping_add(take), Ordering::Release);
        take
    }

    /// The generation at the start of a block — pair with
    /// [`generation_changed`].
    pub fn generation(&self) -> u64 {
        self.inner.generation.load(Ordering::Acquire)
    }

    /// Whether a seek reset landed mid-block: the frames this block took
    /// are stale and the voice should contribute silence for it.
    pub fn generation_changed(&mut self, seen: u64) -> bool {
        let now = self.inner.generation.load(Ordering::Acquire);
        self.seen_generation = now;
        seen != now
    }

    /// Underrun blocks counted so far (diagnostics).
    pub fn underruns(&self) -> u64 {
        self.inner.underruns.load(Ordering::Relaxed)
    }

    /// Filled frames right now (the hysteresis gauge).
    pub fn filled(&self) -> usize {
        self.inner.filled()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn push_pop_round_trip() {
        let (writer, mut reader) = voice_ring(48_000, 2);
        let frames: Vec<f32> = (0..800).map(|i| i as f32).collect();
        assert_eq!(writer.push(&frames), 400);
        let mut out = vec![0f32; 400]; // 200 frames at 2 channels
        assert_eq!(reader.pop(&mut out), 200);
        assert_eq!(&out[..16], &(0..16).map(|i| i as f32).collect::<Vec<_>>());
        assert_eq!(reader.pop(&mut out), 200);
        assert_eq!(&out[..8], &(400..408).map(|i| i as f32).collect::<Vec<_>>());
        // Drained: an underrun is counted, silence implied.
        assert_eq!(reader.pop(&mut out), 0);
        assert_eq!(reader.underruns(), 1);
    }

    #[test]
    fn capacity_is_about_one_second() {
        let (writer, _reader) = voice_ring(44_100, 2);
        // max(rate, 4096) rounded to a power of two = 65536.
        assert_eq!(writer.capacity(), 65_536);
        assert_eq!(writer.channels(), 2);
        let (writer, _) = voice_ring(2_000, 1);
        // Tiny rates still get the 4096 floor.
        assert_eq!(writer.capacity(), 4096);
    }

    #[test]
    fn push_reports_partial_when_full() {
        let (writer, mut reader) = voice_ring_frames(1024, 1);
        let big: Vec<f32> = (0..4096).map(|i| i as f32).collect();
        assert_eq!(writer.push(&big), 1024);
        // Full: nothing more fits.
        assert_eq!(writer.push(&big[..8]), 0);
        let mut out = vec![0f32; 4096];
        assert_eq!(reader.pop(&mut out), 1024);
        assert_eq!(&out[..1024], &big[..1024]);
    }

    #[test]
    fn wraparound_is_seamless() {
        let (writer, mut reader) = voice_ring_frames(64, 1);
        let mut pushed = 0usize;
        let mut popped = 0usize;
        let mut out = vec![0f32; 48];
        for round in 0..16 {
            let chunk: Vec<f32> = (pushed..pushed + 48).map(|i| i as f32 + round as f32 / 100.0).collect();
            pushed += writer.push(&chunk);
            let n = reader.pop(&mut out);
            assert!(n > 0);
            popped += n;
            assert_eq!(pushed, popped, "nothing lost or duplicated");
        }
        assert_eq!(writer.filled(), 0);
    }

    #[test]
    fn reset_drops_queued_audio() {
        let (writer, mut reader) = voice_ring(48_000, 1);
        writer.push(&vec![1f32; 5000]);
        let seen = reader.generation();
        writer.reset();
        assert!(reader.generation_changed(seen));
        assert_eq!(writer.filled(), 0);
        let mut out = vec![0f32; 16];
        assert_eq!(reader.pop(&mut out), 0);
    }

    #[test]
    fn spsc_stress_no_loss_no_duplication() {
        // The classic SPSC invariant under concurrent traffic: every frame
        // the writer pushes comes out exactly once, in order.
        let (writer, mut reader) = voice_ring(48_000, 2);
        let total = 200_000usize;
        let producer = thread::spawn(move || {
            let mut next = 0usize;
            while next < total {
                let chunk: Vec<f32> = (0..64)
                    .map(|i| ((next + i) * 2) as f32)
                    .chain((0..64).map(|i| ((next + i) * 2 + 1) as f32))
                    .collect();
                // interleave
                let mut interleaved = Vec::with_capacity(128);
                for i in 0..64 {
                    interleaved.push(((next + i) * 2) as f32);
                    interleaved.push(((next + i) * 2 + 1) as f32);
                }
                let _ = chunk;
                let pushed = writer.push(&interleaved);
                next += pushed;
            }
        });
        let mut received = Vec::with_capacity(total);
        let mut buf = vec![0f32; 256];
        while received.len() < total * 2 {
            let n = reader.pop(&mut buf);
            if n == 0 {
                std::thread::yield_now();
                continue;
            }
            received.extend_from_slice(&buf[..n * 2]);
        }
        producer.join().unwrap();
        for (frame, pair) in received.chunks_exact(2).enumerate() {
            assert_eq!(pair[0] as usize, frame * 2, "left channel frame {frame}");
            assert_eq!(pair[1] as usize, frame * 2 + 1, "right channel frame {frame}");
        }
    }
}
