//! The userspace USB Audio Class driver — `usb_pass.cpp` ported to rusb.
//!
//! Upstream absorption: `app/src/main/cpp/usb_pass.cpp` (mochy's own USB
//! audio driver, 487 lines) + the Kotlin halves `UsbPass.kt`,
//! `UsbAudioDescriptors.kt`, `UsbDacVolume.kt`, `UsbDacDetails.kt` and
//! the USB parts of `AudioPass.kt` (per-device volume memory, the
//! hand-back delay).
//!
//! Availability (PORT_SPEC §3.2): the `usb-uac` feature, Windows +
//! Linux only, **experimental**. macOS is compiled out by cfg —
//! IOUSBHost's arbitration makes a userspace UAC driver unviable there
//! (blueprint ch. 12, a recorded limitation).
//!
//! Layout:
//! - [`descriptors`] — the UAC 1/2 descriptor parser (pure, always
//!   compiled, `UsbAudioDescriptors.kt` 1:1);
//! - [`volume`] — the DAC's own attenuator over the `UsbTransfer` trait
//!   (pure, always compiled, `UsbDacVolume.kt` 1:1);
//! - [`details`] — `UsbDacDetails`: what a plugged-in DAC says about
//!   itself;
//! - `driver` — the streaming driver itself (feature-gated): interface
//!   claim, UAC clock programming, 8 in-flight ISO URBs, async feedback
//!   endpoint parsing, per-device volume memory.

pub mod descriptors;
pub mod details;
pub mod volume;

#[cfg(all(feature = "usb-uac", any(windows, target_os = "linux")))]
pub mod driver;

// macOS: IOUSBHost arbitration makes the userspace UAC driver unviable;
// the module is compiled out entirely (recorded decision, blueprint
// ch. 12 / PORT_SPEC §8).
