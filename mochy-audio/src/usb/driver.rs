//! The USB direct driver — `usb_pass.cpp` ported to rusb + libusb.
//!
//! "mochy's own usb audio driver: android hands over the dac's file
//! descriptor, this takes the dac from the kernel's driver and streams
//! isochronous packets at the song's own rate, nothing mixed or scaled
//! on the way." The desktop shape: rusb opens the device and runs the
//! control pipe (interface claim, kernel detach on Linux, UAC clock
//! programming, DAC volume); the isochronous machinery rides libusb's
//! async transfer API through `rusb::ffi` — rusb 0.9 has no safe async
//! layer, and libusb's URB model is the same one usbfs gave upstream
//! (submit → completion callback → refill → resubmit).
//!
//! Ported 1:1 where the OS allows:
//! - 8 in-flight ISO URBs of ~8 ms each (`kSlots = 8`, `kUrbUs =
//!   8000`, `kMaxPackets = 64` — "about 64 ms queued at the dac, so a
//!   worker woken late doesn't run it dry");
//! - the fractional frame carry per packet (44.1 kHz in 1 ms packets
//!   comes out exact, `nextPacketFrames`);
//! - the async feedback endpoint: 16.16 (high speed) and 10.14 (full
//!   speed) numbers, candidates {f, 4f, f/4}, anything beyond 2% of
//!   nominal ignored (`readFeedback`);
//! - UAC2 clock programming with the selector pin, UAC1 endpoint rate
//!   requests with read-back (`setRate`);
//! - the played-frames accounting and the hand-back (alt 0, interfaces
//!   released);
//! - the priority ask: best effort, as urgent as the OS allows.
//!
//! Two upstream mechanics change shape on the desktop:
//! - upstream filled a mutex ring from the playback thread
//!   (`usbWrite` into `Stream::ring`); the port's driver pulls each
//!   URB's frames from the [`RenderSource`] directly — the mixer,
//!   whose underrun policy (silence for what isn't there) replaces the
//!   ring's short-queue silence;
//! - upstream's `generation` (the flush rule: "packets already with
//!   the dac finish, but no longer count towards the new start")
//!   fed `usbPlayed`'s position; the port's position is the mixer's
//!   [`PlaybackClock`](mochy_player::PlaybackClock), so the USB side
//!   only tracks submitted-vs-played for the delay report and the
//!   generation bump has no number left to guard. Seeks land in the
//!   mixer's rings; up to 64 ms of pre-seek audio finishing at the DAC
//!   is the same tail upstream left playing.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, mpsc::Sender};
use std::time::Duration;

use rusb::ffi::{self, constants};

use crate::output::{
    EdgeConverter, OpenError, OpenedStream, OutputEvent, OutputFormat, PcmBlock, RenderSource,
    Sharing, StreamReport, StreamRequest,
};

use crate::usb::descriptors::{self as desc, Plan};
use crate::usb::details::UsbVolumeMemory;
use crate::usb::volume::{UsbDacVolume, UsbTransfer};

/// URBs in flight and how long each one plays (usb_pass.cpp:33-36).
const SLOTS: usize = 8;
const URB_US: u32 = 8000;
const MAX_PACKETS: usize = 64;
/// The selector-pin + clock CUR request and the endpoint rate requests
/// (usb_pass.cpp:146-166).
const CUR: u8 = 0x01;

/// libusb's event-wait timeout — 50 ms, the same cadence upstream's
/// `poll` gave the reaper. `libc::timeval` because that is the type
/// libusb1-sys's FFI takes (`long` fields, platform-width).
const REAP_TIMEOUT: libc::timeval = libc::timeval {
    tv_sec: 0,
    tv_usec: 50_000,
};

/// The completion flag every transfer carries in its `user_data` —
/// upstream's `urb->usercontext` let the reaper tell which slot came
/// back; libusb instead calls the callback on the event-handling
/// thread, so the callback only raises the slot's flag and the worker
/// polls it. Boxed: the transfer keeps this address, so it must never
/// move even if the slot does.
type DoneFlag = Box<AtomicBool>;

extern "system" fn completion_flag(transfer: *mut ffi::libusb_transfer) {
    unsafe {
        let flag = (*transfer).user_data as *const AtomicBool;
        if !flag.is_null() {
            (*flag).store(true, Ordering::Release);
        }
    }
}

/// One slot: its libusb transfer, buffer, and how many music frames it
/// carried (the `Slot` struct). The transfer is a plain C struct the
/// libusb event loop touches; it belongs to the worker thread alone
/// after submission.
struct Slot {
    transfer: *mut ffi::libusb_transfer,
    buffer: Vec<u8>,
    done: DoneFlag,
    music_frames: u64,
    in_flight: bool,
}

/// The streaming state the worker owns.
struct Stream {
    handle: rusb::DeviceHandle<rusb::GlobalContext>,
    plan: Plan,
    high_speed: bool,
    period_us: u32,
    packets_per_urb: usize,
    frame_bytes: usize,
    /// The feedback pace (frames per period), carried fractionally.
    pace: f64,
    nominal: f64,
    remainder: f64,
    /// Frames handed to the DAC minus frames it has played back — the
    /// queued audio, which is the callback-to-output delay.
    queued: i64,
}

impl Stream {
    /// Frames for the next packet, carrying the fraction over so
    /// 44.1 kHz in 1 ms packets comes out exact (`nextPacketFrames`).
    fn next_packet_frames(&mut self) -> usize {
        self.remainder += self.pace;
        let whole = self.remainder.floor();
        self.remainder -= whole;
        whole as usize
    }

    /// The DAC says how fast it really plays; both USB speeds' number
    /// formats are tried, and anything far from the nominal rate is
    /// ignored (`readFeedback`).
    fn read_feedback(&mut self, data: &[u8]) {
        let per_frame = if data.len() >= 4 {
            // 16.16 (usb_pass.cpp:199-201).
            f64::from(u32::from_le_bytes([data[0], data[1], data[2], data[3]])) / 65536.0
        } else if data.len() == 3 {
            // 10.14 in three bytes (usb_pass.cpp:202-204): the 24-bit
            // number over 2^14, nothing more.
            f64::from(u32::from_le_bytes([data[0], data[1], data[2], 0])) / 16384.0
        } else {
            return;
        };
        let frames_per_unit = if self.high_speed {
            per_frame * (f64::from(self.period_us) / 125.0)
        } else {
            per_frame * (f64::from(self.period_us) / 1000.0)
        };
        for candidate in [
            frames_per_unit,
            frames_per_unit * 4.0,
            frames_per_unit / 4.0,
        ] {
            if (candidate - self.nominal).abs() <= self.nominal * 0.02 {
                self.pace = candidate;
                return;
            }
        }
    }
}

/// Opens the DAC directly at one song's rate/channels/depth — the
/// `usbOpen` + `UsbPass.open` walk over the plans, best first.
///
/// `memory` carries the per-device volume levels ("the level picked
/// last time comes back, but never louder than the dac's own knob was
/// left").
#[allow(clippy::too_many_arguments)]
pub fn open_usb(
    vid: u16,
    pid: u16,
    dac: &desc::Dac,
    request: &StreamRequest,
    source: Box<dyn RenderSource>,
    events: Sender<OutputEvent>,
    delay_us: Arc<AtomicU64>,
    memory: &UsbVolumeMemory,
    allow_fixed_volume: bool,
) -> Result<OpenedStream, OpenError> {
    let rate = request.rate as i32;
    let channels = request.channels as i32;
    let source_bits = match request.format {
        OutputFormat::I16 => 16,
        OutputFormat::I24 => 24,
        _ => 32,
    };
    // `UsbPass.open`: the plans, capped at 16, no plan = the DAC can't
    // carry this song.
    let candidates: Vec<Plan> = desc::plans(dac, rate, channels, source_bits)
        .into_iter()
        .take(16)
        .collect();
    if candidates.is_empty() {
        return Err(OpenError::Failed(format!(
            "the dac has no setting for {rate} Hz {source_bits}-bit"
        )));
    }
    // The volume unit must exist unless fixed volume is allowed
    // ("the dac has no volume of its own", UsbPass.open).
    if dac.volume.is_none() && !allow_fixed_volume {
        return Err(OpenError::Failed("the dac has no volume of its own".into()));
    }
    let mut handle = rusb::DeviceList::new()
        .map_err(|e| OpenError::Failed(e.to_string()))?
        .iter()
        .find(|device| {
            device
                .device_descriptor()
                .map(|d| d.vendor_id() == vid && d.product_id() == pid)
                .unwrap_or(false)
        })
        .ok_or_else(|| OpenError::Failed("the dac is not attached".into()))?
        .open()
        .map_err(|e| OpenError::Failed(e.to_string()))?;

    // Walk the plans best-first; the first that configures wins
    // (usbOpen's loop + configure(), each refusal logged and the walk
    // carrying on with the same device).
    let mut stream = None;
    for plan in &candidates {
        match configure(handle, plan, dac.bcd_usb) {
            Ok(configured) => {
                stream = Some(configured);
                break;
            }
            Err((_why, returned)) => {
                handle = returned;
            }
        }
    }
    let mut stream = stream
        .ok_or_else(|| OpenError::Failed("the dac wouldn't run at this song's rate".into()))?;

    // The DAC's own volume: turned down before the first sound, the
    // remembered level riding the knob's cap (UsbDacVolume.open +
    // rememberedDb).
    let mut opened_volume = None;
    if let Some(unit) = dac.volume.clone() {
        let mut bridge = RusbTransfer {
            handle: &mut stream.handle,
        };
        opened_volume = UsbDacVolume::open(&mut bridge, unit).map(|mut v| {
            if let Some(db) = memory.level_for(vid, pid, v.original_db()) {
                v.set(&mut bridge, db);
            }
            v
        });
    }
    if opened_volume.is_none() && !allow_fixed_volume {
        return Err(OpenError::Failed(
            "the dac's volume couldn't be turned down first".into(),
        ));
    }

    let report = StreamReport {
        sharing: Sharing::Exclusive,
        rate: request.rate,
        channels: request.channels,
        format: request.format,
        hardware: None,
        // A second of audio waited in upstream's ring
        // (usbOpen:392-393); the mixer's per-voice ring holds it now.
        buffer_frames: (request.rate as usize).max(4096),
        device_id: Some(format!("{vid:04x}:{pid:04x}")),
    };
    let stopped = Arc::new(AtomicBool::new(false));
    let thread_stopped = stopped.clone();
    let thread_delay = delay_us.clone();
    let bits = (stream.plan.subslot * 8) as u32;
    let format = request.format;
    let out_channels = request.channels;
    let thread = std::thread::Builder::new()
        .name("mochy-usb-dac".into())
        .spawn(move || {
            // Best effort: a late packet is a click, so the worker asks
            // to be scheduled first, as urgently as the OS allows an
            // app (usb_pass.cpp:283 — setpriority; the desktop leaves
            // thread priority alone, recorded deviation).
            run_worker(
                stream,
                source,
                format,
                bits,
                source_bits as u32,
                out_channels,
                thread_stopped,
                thread_delay,
                events,
            );
        })
        .map_err(|e| OpenError::Failed(e.to_string()))?;
    let join_stopped = stopped.clone();
    Ok(OpenedStream::from_parts(
        report,
        delay_us,
        Box::new(move || {
            join_stopped.store(true, Ordering::Release);
            let _ = thread.join();
        }),
        stopped,
    ))
}

/// Claims the interfaces, sets the alt setting, programs the clock —
/// `Stream::claim/giveBack/setAlt/setRate/configure`. The handle rides
/// through the `Err` so the next plan walks the same open device.
fn configure(
    mut handle: rusb::DeviceHandle<rusb::GlobalContext>,
    plan: &Plan,
    bcd_usb: i32,
) -> Result<Stream, (String, rusb::DeviceHandle<rusb::GlobalContext>)> {
    if let Err(why) = claim_and_start(&mut handle, plan) {
        return Err((why, handle));
    }

    // The bus speed decides the period and the feedback number format
    // (usbOpen's USBDEVFS_GET_SPEED / bcdUsb fallback).
    let high_speed = match handle.device().speed() {
        rusb::Speed::High | rusb::Speed::Super | rusb::Speed::SuperPlus => true,
        rusb::Speed::Low | rusb::Speed::Full => false,
        // Unknown and any future speed: the descriptor's bcdUSB decides
        // (usbOpen's fallback).
        _ => bcd_usb >= 0x0200,
    };
    let interval = plan.interval.max(1);
    let period = (if high_speed { 125u32 } else { 1000u32 }) << (interval - 1);
    let per_packet = f64::from(plan.rate) * f64::from(period) / 1e6;
    let frame_bytes = (plan.channels * plan.subslot) as usize;
    // Room for one frame more than nominal, which is as far as a DAC's
    // feedback ever asks (usb_pass.cpp:172-173).
    if ((per_packet.ceil() as usize) + 1) * frame_bytes > plan.max_packet as usize {
        return Err(("packets too small for the rate".into(), handle));
    }
    let packets_per_urb = (URB_US / period).clamp(1, MAX_PACKETS as u32) as usize;
    Ok(Stream {
        handle,
        plan: plan.clone(),
        high_speed,
        period_us: period,
        packets_per_urb,
        frame_bytes,
        pace: per_packet,
        nominal: per_packet,
        remainder: 0.0,
        queued: 0,
    })
}

/// The claim + alt + clock walk `configure()` ran before a plan was
/// accepted (usb_pass.cpp:168-178).
fn claim_and_start(
    handle: &mut rusb::DeviceHandle<rusb::GlobalContext>,
    plan: &Plan,
) -> Result<(), String> {
    // The kernel's USB audio driver lets go, and the interface is ours
    // until we hand it back (USBDEVFS_DISCONNECT_CLAIM upstream; the
    // libusb way: detach the kernel driver, then claim).
    for iface in [plan.control_interface, plan.streaming_interface] {
        let iface = u8::try_from(iface).map_err(|e| e.to_string())?;
        #[cfg(target_os = "linux")]
        {
            if handle.kernel_driver_active(iface).unwrap_or(false) {
                handle
                    .detach_kernel_driver(iface)
                    .map_err(|e| format!("detach {iface}: {e}"))?;
            }
        }
        handle
            .claim_interface(iface)
            .map_err(|e| format!("claim {iface}: {e}"))?;
    }
    // Alt 0 first (idle), clock, then the real alt — the same order
    // configure() uses (usb_pass.cpp:174-178).
    set_alt(handle, plan.streaming_interface, 0)?;
    if plan.uac2 {
        set_rate(handle, plan)?;
    }
    set_alt(handle, plan.streaming_interface, plan.alt)?;
    if !plan.uac2 {
        set_rate(handle, plan)?;
    }
    Ok(())
}

fn set_alt(
    handle: &mut rusb::DeviceHandle<rusb::GlobalContext>,
    iface: i32,
    alt: i32,
) -> Result<(), String> {
    handle
        .set_alternate_setting(
            u8::try_from(iface).map_err(|e| e.to_string())?,
            u8::try_from(alt).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())
}

/// Sets the DAC's clock to the song's rate and reads it back: a DAC
/// that quietly stays at another rate would play the song too fast or
/// slow (`setRate`, usb_pass.cpp:146-166).
fn set_rate(
    handle: &mut rusb::DeviceHandle<rusb::GlobalContext>,
    plan: &Plan,
) -> Result<(), String> {
    let control = u8::try_from(plan.control_interface).map_err(|e| e.to_string())?;
    if plan.uac2 {
        if plan.selector_id > 0 && plan.selector_pin > 0 {
            let pin = [u8::try_from(plan.selector_pin).map_err(|e| e.to_string())?];
            let _ = handle.write_control(
                0x21,
                CUR,
                0x0100,
                ((plan.selector_id as u16) << 8) | u16::from(control),
                &pin,
                Duration::from_secs(1),
            );
        }
        let index = ((plan.clock_id as u16) << 8) | u16::from(control);
        let wanted = (plan.rate as u32).to_le_bytes();
        let mut now = [0u8; 4];
        if handle.read_control(0xA1, CUR, 0x0100, index, &mut now, Duration::from_secs(1)) == Ok(4)
            && now == wanted
        {
            return Ok(());
        }
        handle
            .write_control(0x21, CUR, 0x0100, index, &wanted, Duration::from_secs(1))
            .map_err(|e| e.to_string())?;
        let mut now = [0u8; 4];
        if handle.read_control(0xA1, CUR, 0x0100, index, &mut now, Duration::from_secs(1)) == Ok(4)
            && now == wanted
        {
            Ok(())
        } else {
            Err("the clock refused the rate".into())
        }
    } else if !plan.rate_control {
        Ok(())
    } else {
        let wanted = [
            (plan.rate & 0xFF) as u8,
            ((plan.rate >> 8) & 0xFF) as u8,
            ((plan.rate >> 16) & 0xFF) as u8,
        ];
        let endpoint = plan.endpoint as u16;
        let _ = handle.write_control(0x22, CUR, 0x0100, endpoint, &wanted, Duration::from_secs(1));
        // Some UAC1 DACs take the rate but can't say it back; only a
        // different answer counts as refused.
        let mut now = [0u8; 3];
        match handle.read_control(
            0xA2,
            0x81,
            0x0100,
            endpoint,
            &mut now,
            Duration::from_secs(1),
        ) {
            Ok(3) if now != wanted => Err("the endpoint refused the rate".into()),
            _ => Ok(()),
        }
    }
}

/// The rusb control pipe as a [`UsbTransfer`] for the volume code.
struct RusbTransfer<'a> {
    handle: &'a mut rusb::DeviceHandle<rusb::GlobalContext>,
}

impl UsbTransfer for RusbTransfer<'_> {
    fn transfer(
        &mut self,
        request_type: i32,
        request: i32,
        value: i32,
        index: i32,
        data: &mut [u8],
    ) -> i32 {
        let result = if request_type & 0x80 != 0 {
            self.handle.read_control(
                request_type as u8,
                request as u8,
                value as u16,
                index as u16,
                data,
                Duration::from_secs(1),
            )
        } else {
            self.handle.write_control(
                request_type as u8,
                request as u8,
                value as u16,
                index as u16,
                data,
                Duration::from_secs(1),
            )
        };
        match result {
            Ok(moved) => moved as i32,
            Err(_) => -1,
        }
    }
}

/// The fill: frames from the render source, sized per packet with the
/// fractional carry, silence for what isn't there (the mixer's
/// underrun policy feeding the DAC's clock-keeping silence,
/// usb_pass.cpp:217-253).
#[allow(clippy::too_many_arguments)]
fn fill_slot(
    stream: &mut Stream,
    slot: &mut Slot,
    source: &mut Box<dyn RenderSource>,
    converter: &mut EdgeConverter,
    block: &mut PcmBlock,
    scratch: &mut Vec<f32>,
    channels: usize,
    dev: *mut ffi::libusb_device_handle,
) {
    let packets = stream.packets_per_urb;
    let frame_bytes = stream.frame_bytes;
    let mut lengths = Vec::with_capacity(packets);
    let mut total = 0usize;
    for _ in 0..packets {
        let frames = stream.next_packet_frames();
        let bytes = (frames * frame_bytes)
            .min((stream.plan.max_packet as usize / frame_bytes) * frame_bytes);
        lengths.push(bytes as u32);
        total += bytes;
    }
    let frames_needed = total / frame_bytes;
    scratch.clear();
    scratch.resize(frames_needed * channels, 0.0);
    source.render(scratch, channels);
    converter.convert(scratch, block);
    let bytes = block.to_le_bytes();
    let copy = bytes.len().min(slot.buffer.len());
    slot.buffer[..copy].copy_from_slice(&bytes[..copy]);
    // A short queue or a pause goes out as silence, so the DAC's
    // clock never loses lock.
    slot.buffer[copy..].fill(0);
    slot.music_frames = (copy / frame_bytes) as u64;
    stream.queued += slot.music_frames as i64;
    unsafe {
        prepare_iso(
            slot.transfer,
            dev,
            stream.plan.endpoint as u8,
            &slot.buffer,
            &lengths,
            &slot.done,
        );
    }
}

/// Prepares one ISO transfer for submission — type ISO, the endpoint,
/// one buffer, per-packet lengths (the fill's URB setup). The timeout
/// stays 0: an isochronous transfer is never timed out, it plays when
/// it plays (upstream set none either — USBDEVFS_URB_ISO_ASAP).
unsafe fn prepare_iso(
    transfer: *mut ffi::libusb_transfer,
    dev: *mut ffi::libusb_device_handle,
    endpoint: u8,
    buffer: &[u8],
    lengths: &[u32],
    done: &AtomicBool,
) {
    let total: u32 = lengths.iter().sum();
    unsafe {
        ffi::libusb_fill_iso_transfer(
            transfer,
            dev,
            endpoint,
            buffer.as_ptr() as *mut u8,
            total as i32,
            lengths.len() as i32,
            completion_flag,
            done as *const AtomicBool as *mut c_void,
            0,
        );
        // Per-packet lengths carry the fractional frame count — the same
        // length for every packet would drift 44.1 kHz.
        for (packet, length) in lengths.iter().enumerate() {
            (*transfer).iso_packet_desc.as_mut_ptr().add(packet).write(
                ffi::libusb_iso_packet_descriptor {
                    length: *length,
                    actual_length: 0,
                    status: 0,
                },
            );
        }
    }
}

/// Prepares the feedback transfer: one packet, the DAC's reported
/// pace (`fillFeedback`).
unsafe fn prepare_feedback(slot: &mut Slot, dev: *mut ffi::libusb_device_handle, endpoint: u8) {
    let length = slot.buffer.len();
    unsafe {
        ffi::libusb_fill_iso_transfer(
            slot.transfer,
            dev,
            endpoint,
            slot.buffer.as_ptr() as *mut u8,
            length as i32,
            1,
            completion_flag,
            &*slot.done as *const AtomicBool as *mut c_void,
            0,
        );
        (*slot.transfer).iso_packet_desc.as_mut_ptr().add(0).write(
            ffi::libusb_iso_packet_descriptor {
                length: length as u32,
                actual_length: 0,
                status: 0,
            },
        );
    }
}

/// The worker: submits the 8 URBs, reaps completions, refills, parses
/// feedback — `Stream::run` (usb_pass.cpp:281-342) on libusb's event
/// loop.
#[allow(clippy::too_many_arguments)]
fn run_worker(
    mut stream: Stream,
    mut source: Box<dyn RenderSource>,
    format: OutputFormat,
    bits: u32,
    source_bits: u32,
    channels: usize,
    stopped: Arc<AtomicBool>,
    delay_us: Arc<AtomicU64>,
    events: Sender<OutputEvent>,
) {
    let dev = stream.handle.as_raw();
    let mut converter = EdgeConverter::new_exact(format, bits, source_bits);
    let mut block = PcmBlock::default();
    let mut scratch = Vec::new();
    let feedback_endpoint = stream.plan.feedback_endpoint;
    let packets = stream.packets_per_urb;
    let max_packet = stream.plan.max_packet as usize;

    let mut slots: Vec<Slot> = Vec::with_capacity(SLOTS);
    unsafe {
        for _ in 0..SLOTS {
            let Some(slot) = new_slot(packets, max_packet) else {
                return;
            };
            slots.push(slot);
        }
    }
    let mut feedback = None;
    if feedback_endpoint >= 0 && stream.plan.feedback_max_packet > 0 {
        unsafe {
            feedback = new_slot(1, stream.plan.feedback_max_packet as usize);
        }
    }

    unsafe {
        for slot in slots.iter_mut() {
            fill_slot(
                &mut stream,
                slot,
                &mut source,
                &mut converter,
                &mut block,
                &mut scratch,
                channels,
                dev,
            );
            if ffi::libusb_submit_transfer(slot.transfer) != 0 {
                fail(&events, "the dac refused the stream");
                return;
            }
            slot.in_flight = true;
        }
        if let Some(feedback_slot) = feedback.as_mut() {
            prepare_feedback(feedback_slot, dev, feedback_endpoint as u8);
            if ffi::libusb_submit_transfer(feedback_slot.transfer) != 0 {
                // Feedback is best-effort: the nominal pace carries on.
                feedback_slot.in_flight = false;
            } else {
                feedback_slot.in_flight = true;
            }
        }
    }

    // The reap loop: wait for events with a 50 ms timeout (upstream's
    // poll), then service every completed transfer (the REAPURBNDELAY
    // walk).
    loop {
        if stopped.load(Ordering::Acquire) {
            break;
        }
        let mut completed = 0i32;
        unsafe {
            ffi::libusb_handle_events_timeout_completed(
                std::ptr::null_mut(),
                &REAP_TIMEOUT,
                &mut completed,
            );
        }
        let mut device_lost = false;
        unsafe {
            for slot in slots.iter_mut() {
                if !slot.in_flight || !slot.done.load(Ordering::Acquire) {
                    continue;
                }
                slot.done.store(false, Ordering::Release);
                slot.in_flight = false;
                let status = (*slot.transfer).status;
                if status == constants::LIBUSB_TRANSFER_NO_DEVICE
                    || status == constants::LIBUSB_TRANSFER_ERROR
                {
                    device_lost = true;
                    break;
                }
                // The frames this URB carried are in the DAC's ears;
                // what stays queued is the delay.
                stream.queued -= slot.music_frames as i64;
                let rate = u64::from(stream.plan.rate as u32).max(1);
                delay_us.store(
                    stream.queued.max(0) as u64 * 1_000_000 / rate,
                    Ordering::Release,
                );
                if !stopped.load(Ordering::Acquire) {
                    fill_slot(
                        &mut stream,
                        slot,
                        &mut source,
                        &mut converter,
                        &mut block,
                        &mut scratch,
                        channels,
                        dev,
                    );
                    if ffi::libusb_submit_transfer(slot.transfer) != 0 {
                        device_lost = true;
                        break;
                    }
                    slot.in_flight = true;
                }
            }
            if let Some(feedback_slot) = feedback.as_mut()
                && feedback_slot.in_flight
                && feedback_slot.done.load(Ordering::Acquire)
            {
                feedback_slot.done.store(false, Ordering::Release);
                feedback_slot.in_flight = false;
                let status = (*feedback_slot.transfer).status;
                let actual = iso_actual_length(feedback_slot.transfer, 0);
                if status == constants::LIBUSB_TRANSFER_COMPLETED && actual > 0 {
                    let seen = actual.min(feedback_slot.buffer.len());
                    stream.read_feedback(&feedback_slot.buffer[..seen]);
                }
                prepare_feedback(feedback_slot, dev, feedback_endpoint as u8);
                if ffi::libusb_submit_transfer(feedback_slot.transfer) == 0 {
                    feedback_slot.in_flight = true;
                }
            }
        }
        if device_lost {
            fail(&events, "the dac was unplugged");
            break;
        }
    }

    // The kernel still owns every URB in flight; each is cancelled and
    // collected before its memory goes (usb_pass.cpp:327-341).
    unsafe {
        for slot in slots.iter() {
            if slot.in_flight {
                ffi::libusb_cancel_transfer(slot.transfer);
            }
        }
        if let Some(feedback_slot) = feedback.as_ref()
            && feedback_slot.in_flight
        {
            ffi::libusb_cancel_transfer(feedback_slot.transfer);
        }
        // Upstream gave the reaper 200 tries of 5 ms; the same budget
        // here, events flowing through the timeout wait.
        for _ in 0..200 {
            let mut completed = 0i32;
            ffi::libusb_handle_events_timeout_completed(
                std::ptr::null_mut(),
                &REAP_TIMEOUT,
                &mut completed,
            );
            for slot in slots.iter_mut() {
                if slot.in_flight && slot.done.load(Ordering::Acquire) {
                    slot.done.store(false, Ordering::Release);
                    slot.in_flight = false;
                }
            }
            if let Some(feedback_slot) = feedback.as_mut()
                && feedback_slot.in_flight
                && feedback_slot.done.load(Ordering::Acquire)
            {
                feedback_slot.done.store(false, Ordering::Release);
                feedback_slot.in_flight = false;
            }
            let any = slots.iter().any(|slot| slot.in_flight)
                || feedback.as_ref().is_some_and(|slot| slot.in_flight);
            if !any {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        for slot in slots {
            ffi::libusb_free_transfer(slot.transfer);
        }
        if let Some(feedback_slot) = feedback {
            ffi::libusb_free_transfer(feedback_slot.transfer);
        }
        // Back to alt 0 and the interfaces released — the DAC is an
        // ordinary OS output once more (close() → giveBack()).
        let _ = set_alt(&mut stream.handle, stream.plan.streaming_interface, 0);
        let _ = stream
            .handle
            .release_interface(u8::try_from(stream.plan.control_interface).unwrap_or(0));
        let _ = stream
            .handle
            .release_interface(u8::try_from(stream.plan.streaming_interface).unwrap_or(0));
    }
}

/// Allocates one slot's transfer with room for its ISO packets
/// (`Slot::Slot` — calloc of the urb plus its descriptors). None when
/// the allocation refused.
unsafe fn new_slot(packets: usize, packet_bytes: usize) -> Option<Slot> {
    let transfer = unsafe { ffi::libusb_alloc_transfer(packets as i32) };
    if transfer.is_null() {
        return None;
    }
    Some(Slot {
        transfer,
        buffer: vec![0u8; packets.saturating_mul(packet_bytes)],
        done: Box::new(AtomicBool::new(false)),
        music_frames: 0,
        in_flight: false,
    })
}

fn fail(events: &Sender<OutputEvent>, _why: &str) {
    let _ = events.send(OutputEvent::DeviceLost);
}

/// The byte count one packet actually moved (`iso_frame_desc[i].
/// actual_length`).
unsafe fn iso_actual_length(transfer: *mut ffi::libusb_transfer, packet: usize) -> usize {
    unsafe {
        (*transfer)
            .iso_packet_desc
            .as_ptr()
            .add(packet)
            .read()
            .actual_length as usize
    }
}
