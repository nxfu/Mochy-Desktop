//! The hotplug-resume acceptance test: a device removed mid-song → the
//! stream re-opens on what remains and playback resumes at the same
//! media position (the AudioTrack-failure recovery path ported as
//! reinit-at-position; blueprint ch. 11).
//!
//! Drives the real [`mochy_player::PlayerCore`] through the real
//! [`mochy_audio::output::OutputRouter`] over the mock backend — the
//! whole device-loss scenario without hardware.

use std::sync::Arc;
use std::time::Duration;

use mochy_audio::output::mock::{MockBackend, MockDevice};
use mochy_audio::output::{OutputEvent, OutputRouter, Sharing, StreamRequest};
use mochy_player::{FrameSource, PlayerCore};

/// A long deterministic ramp: value = sample × 0.00002 (sub-ceiling).
struct LongRamp {
    next: u64,
    end: u64,
}

impl FrameSource for LongRamp {
    fn media_rate(&self) -> u32 {
        48_000
    }
    fn channels(&self) -> usize {
        1
    }
    fn next_block(&mut self, out: &mut Vec<f32>) -> Option<()> {
        if self.next >= self.end {
            return None;
        }
        out.clear();
        let take = 1024.min((self.end - self.next) as usize);
        for i in 0..take {
            out.push((self.next + i as u64) as f32 * 0.00002);
        }
        self.next += take as u64;
        Some(())
    }
    fn seek(&mut self, to_sample: u64) -> u64 {
        self.next = to_sample.min(self.end);
        self.next
    }
    fn position(&self) -> u64 {
        self.next
    }
}

#[test]
fn device_removed_mid_song_reopens_and_preserves_position() {
    let backend = Arc::new(MockBackend::default());
    backend.add(MockDevice {
        id: "dac".into(),
        exact: true,
    });
    // A second device to fall back onto.
    backend.add(MockDevice {
        id: "speakers".into(),
        exact: true,
    });
    let router = OutputRouter::new(backend.clone());
    let (core, mut mixer) = PlayerCore::new(48_000, 1);

    // Play a long song.
    let slot = core
        .play(Box::new(LongRamp {
            next: 0,
            end: 2_000_000,
        }))
        .expect("a free voice");
    core.set_primary(slot);

    // Open the output on the dac.
    let request = StreamRequest {
        rate: 48_000,
        channels: 1,
        format: mochy_audio::output::OutputFormat::F32,
        exclusive: false,
        device: Some("dac".into()),
    };
    let report = router
        .ensure(&request, || {
            // The router owns a render source; for this test the mixer
            // is rendered directly below instead (the mock backend runs
            // no render thread), so the source here is a stub the mock
            // never calls.
            struct Stub;
            impl mochy_audio::output::RenderSource for Stub {
                fn render(&mut self, out: &mut [f32], _channels: usize) {
                    out.fill(0.0);
                }
            }
            Box::new(Stub)
        })
        .expect("open on the dac");
    assert_eq!(report.device_id.as_deref(), Some("dac"));

    // Let the song play for a while.
    std::thread::sleep(Duration::from_millis(60));
    let mut out = vec![0f32; 4800];
    for _ in 0..4 {
        mixer.render(&mut out);
    }
    let before = core.clock().audio_position();
    assert!(before.0 > 0.05, "nothing played: {before:?}");

    // The dac is yanked mid-song.
    backend.lose_device(&report);
    let event = router
        .events()
        .recv_timeout(Duration::from_secs(2))
        .expect("the loss must be signaled");
    assert_eq!(event, OutputEvent::DeviceLost);
    assert!(router.handle_device_lost());

    // Recovery: re-open on the speakers and resume at position.
    let fallback_request = StreamRequest {
        device: Some("speakers".into()),
        ..request
    };
    let report2 = router
        .ensure(&fallback_request, || {
            struct Stub;
            impl mochy_audio::output::RenderSource for Stub {
                fn render(&mut self, out: &mut [f32], _channels: usize) {
                    out.fill(0.0);
                }
            }
            Box::new(Stub)
        })
        .expect("reopen on the speakers");
    assert_eq!(report2.device_id.as_deref(), Some("speakers"));
    assert_eq!(report2.sharing, Sharing::Shared);

    core.resume_at_position();
    // The decode task refills; rendering continues from the same media
    // position (no rewind to zero, no jump forward past the song).
    std::thread::sleep(Duration::from_millis(150));
    mixer.render(&mut out);
    let after = core.clock().audio_position();
    assert!(
        after.0 >= before.0 && after.0 < before.0 + 2.0,
        "position not preserved across the reopen: {before:?} → {after:?}"
    );
    // And the song is actually still sounding: the first frame of the
    // resumed render picks up exactly at the resume point (the clock
    // then advances by the rendered block on top of it).
    let value_now = out[0];
    let expected_at_resume = before.0 * 48_000.0 * 0.00002;
    assert!(
        (value_now - expected_at_resume as f32).abs() < 0.02,
        "audio resumed elsewhere: {value_now} vs {expected_at_resume}"
    );
    assert!(
        after.0 > before.0,
        "the clock must advance after resuming: {before:?} → {after:?}"
    );

    core.shutdown();
}
