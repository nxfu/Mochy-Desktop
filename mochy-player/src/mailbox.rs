//! The parameter mailbox — atomic snapshots swapped between blocks.
//!
//! The blueprint's concurrency rule (ch. 11 / PORT_SPEC §3.1.4): "the
//! audio callback owns no locks (mixer pulls from lock-free rings; params
//! are atomic snapshots swapped between blocks)". This is that second
//! half: a single-slot handoff where
//!
//! - the **control side** (any app thread, through `&self`) publishes a
//!   `Copy` snapshot — latest-wins, exactly like the upstream DAC-volume
//!   coalescing ("a dragged slider sends many, only the latest goes
//!   out", AudioPass.kt:227-235);
//! - the **audio callback** adopts the snapshot **between blocks**, by
//!   CAS-ing the state machine and memcpy-ing the value out. The RT path
//!   is bounded: two CAS attempts and one copy, no spinning, no locks.
//!
//! The state machine (`EMPTY → READY → TAKEN/EMPTY`, with `WRITING`
//! guarding the slot) makes torn reads impossible: the producer only
//! writes the slot in `WRITING`, the consumer only reads it in
//! `READING`, and those states are mutually exclusive by construction.

use std::cell::UnsafeCell;
use std::mem::MaybeUninit;
use std::sync::atomic::{AtomicU8, Ordering};

const EMPTY: u8 = 0;
const READY: u8 = 1;
const WRITING: u8 = 2;
const READING: u8 = 3;

/// A latest-wins single-slot mailbox of `T: Copy`.
///
/// # Real-time safety
/// [`ParamMailbox::adopt`] never blocks, never spins, never allocates,
/// never drops anything: worst case it tries one CAS and gives up (the
/// next block picks the value up). The control side may briefly spin
/// while the callback is mid-adoption — that is the app thread's time,
/// not the callback's.
pub struct ParamMailbox<T> {
    slot: UnsafeCell<MaybeUninit<T>>,
    state: AtomicU8,
}

impl<T: Copy> std::fmt::Debug for ParamMailbox<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ParamMailbox")
            .field("state", &self.state.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

// Safety: the slot is only ever accessed under the WRITING (single
// producer) or READING (single consumer) states, which the state machine
// makes mutually exclusive; the callback half never crosses an await
// point (there are none) and the control half synchronizes through the
// same atomic. Sharing across threads is the entire point.
unsafe impl<T: Copy + Send> Send for ParamMailbox<T> {}
unsafe impl<T: Copy + Send> Sync for ParamMailbox<T> {}

impl<T: Copy> ParamMailbox<T> {
    /// An empty mailbox.
    pub const fn new() -> Self {
        ParamMailbox {
            slot: UnsafeCell::new(MaybeUninit::uninit()),
            state: AtomicU8::new(EMPTY),
        }
    }

    /// Publishes a snapshot; the previous unread one (if any) is
    /// overwritten — latest wins. Control-side only.
    ///
    /// The take is a **CAS from an observed state** (never a blind
    /// restore): restoring a state we merely *observed* could land after
    /// the consumer already cleared it, re-poisoning the machine with a
    /// `READING` nobody will ever clear (found by the stress test — the
    /// ghost-write interleaving).
    pub fn publish(&self, value: T) {
        loop {
            // EMPTY and READY are takeable (an unread snapshot is stale
            // by definition — latest wins).
            if self
                .state
                .compare_exchange(EMPTY, WRITING, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
            {
                break;
            }
            if self
                .state
                .compare_exchange(READY, WRITING, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
            {
                break;
            }
            // READING (the consumer is mid-adoption) or a competing
            // WRITING: breathe and retry — the consumer holds the slot
            // for one memcpy.
            std::thread::yield_now();
        }
        unsafe {
            // Safety: WRITING is exclusive to us; no reader touches the slot
            // in this state.
            (*self.slot.get()).write(value);
        }
        self.state.store(READY, Ordering::Release);
    }

    /// Adopts the pending snapshot, if any — call **between blocks** from
    /// the audio callback. Returns `None` when nothing new is pending or
    /// the slot is contended (retry next block).
    pub fn adopt(&self) -> Option<T> {
        if self
            .state
            .compare_exchange(READY, READING, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            return None;
        }
        let value = unsafe {
            // Safety: READING is exclusive to the consumer; the producer
            // parks in publish() while this state is held.
            (*self.slot.get()).assume_init()
        };
        self.state.store(EMPTY, Ordering::Release);
        Some(value)
    }

    /// Whether a snapshot is waiting (diagnostics/tests).
    pub fn is_pending(&self) -> bool {
        self.state.load(Ordering::Acquire) == READY
    }
}

impl<T: Copy> Default for ParamMailbox<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[derive(Clone, Copy, PartialEq, Debug)]
    struct Params {
        master_gain: f32,
        ceiling: f32,
        muted: bool,
        seq: u64,
    }

    #[test]
    fn publish_then_adopt() {
        let mailbox: ParamMailbox<Params> = ParamMailbox::new();
        assert!(mailbox.adopt().is_none());
        mailbox.publish(Params {
            master_gain: 0.5,
            ceiling: 0.99,
            muted: false,
            seq: 1,
        });
        assert!(mailbox.is_pending());
        let adopted = mailbox.adopt().unwrap();
        assert_eq!(adopted.seq, 1);
        assert!(!mailbox.is_pending());
        assert!(mailbox.adopt().is_none());
    }

    #[test]
    fn latest_wins() {
        let mailbox: ParamMailbox<Params> = ParamMailbox::new();
        for seq in 1..=10 {
            mailbox.publish(Params {
                master_gain: 1.0,
                ceiling: 1.0,
                muted: false,
                seq,
            });
        }
        assert_eq!(mailbox.adopt().unwrap().seq, 10);
    }

    #[test]
    fn concurrent_publish_adopt_never_tears() {
        // The stress contract: whatever the callback adopts is always a
        // WHOLE snapshot — the correlated fields never mix across
        // publishes. Runs a monotonically increasing seq with a checksum
        // derived from it. The producer runs on its own thread against a
        // leaked handle — the shape is "one control thread, one adopting
        // reader".
        let mailbox: &'static ParamMailbox<Params> = Box::leak(Box::new(ParamMailbox::new()));
        let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let done_flag = done.clone();
        let producer = thread::spawn(move || {
            for seq in 1..50_000u64 {
                mailbox.publish(Params {
                    master_gain: seq as f32 / 50_000.0,
                    ceiling: 0.99,
                    muted: seq % 2 == 0,
                    seq,
                });
                // Pace: a real control thread publishes on user/setting
                // cadence, not a tight loop — and the test must adopt
                // even under a loaded test runner.
                if seq % 25 == 0 {
                    std::thread::yield_now();
                }
            }
            done_flag.store(true, Ordering::Release);
        });
        let mut adopted = 0usize;
        let mut last_seq = 0u64;
        // Adopt until the producer finishes (plus a drain), yielding on
        // idle so the producer gets the core.
        while !done.load(Ordering::Acquire) || mailbox.is_pending() {
            if let Some(params) = mailbox.adopt() {
                // Whole-snapshot invariant: gain is derived from seq.
                assert_eq!(
                    params.master_gain,
                    params.seq as f32 / 50_000.0,
                    "torn snapshot"
                );
                assert_eq!(params.muted, params.seq % 2 == 0, "torn snapshot");
                assert!(params.seq >= last_seq, "went backwards");
                last_seq = params.seq;
                adopted += 1;
            } else {
                std::thread::yield_now();
            }
        }
        producer.join().unwrap();
        assert!(adopted > 100, "adoption starved: {adopted}");
    }
}
