//! `UsbDacDetails` — what a plugged-in USB DAC says about itself.
//!
//! Port of `playback/audio/UsbDacDetails.kt` (upstream pin `bde01cb4`):
//! the USB side from its descriptors, the audio side from the OS. The
//! struct + derivation logic are pure so they compile everywhere; the
//! rusb-based reader ([`read_rusb`]) sits behind `usb-uac` on Windows
//! and Linux (upstream read it through `UsbManager`'s already-parsed
//! descriptors, needing no permission — the desktop reads the raw
//! descriptor bytes, which rusb can do unclaimed).

use crate::usb::descriptors;

/// What a plugged-in USB DAC says about itself (`UsbDacDetails`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UsbDacDetails {
    pub name: String,
    pub maker: Option<String>,
    pub vendor_id: Option<u16>,
    pub product_id: Option<u16>,
    pub firmware: Option<String>,
    /// 1, 2 or 3, from the audio interface's protocol.
    pub uac_version: Option<u8>,
    /// True when the DAC runs on its own clock and tells the host how
    /// fast to send.
    pub async_clock: Option<bool>,
    pub max_power_ma: Option<u16>,
    pub has_mic: bool,
    /// Empty when the OS says it takes any rate.
    pub rates: Vec<u32>,
    pub bit_depths: Vec<u32>,
    /// What the OS mixer can hand it untouched (the Android-14 analog;
    /// the desktop exclusive modes cover this role).
    pub bit_perfect: Vec<DacFormat>,
    /// False while our own driver holds the device — the OS doesn't
    /// list it as an output then.
    pub os_output: bool,
}

/// One rate/depth pair the DAC takes untouched (`DacFormat`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DacFormat {
    pub sample_rate: u32,
    pub bits: u32,
}

/// Derives the details from a device's raw descriptor bytes + the OS
/// audio side (`UsbDacReader.read`'s pure half — upstream folded the
/// `AudioManager` answers in here, the port takes them as parameters).
///
/// * `os_rates` / `os_bit_depths` — what the OS audio layer reports
///   (empty when it takes any rate);
/// * `os_output` — whether the OS currently lists the DAC as an output.
pub fn read_from_descriptors(
    raw_descriptors: &[u8],
    os_rates: &[u32],
    os_bit_depths: &[u32],
    os_output: bool,
) -> UsbDacDetails {
    let dac = descriptors::parse(raw_descriptors);
    // A mic shows up as an iso IN endpoint carrying data, not the clock
    // feedback (UsbDacDetails.kt:55-61).
    let has_mic = has_microphone(raw_descriptors);
    // The async clock: any OUT iso endpoint typed async
    // (UsbDacDetails.kt:72).
    let async_clock = out_isochronous_endpoints(raw_descriptors)
        .first()
        .map(|attributes| (attributes >> 2) & 3 == 1);
    let bit_perfect = dac
        .settings
        .iter()
        .map(|setting| DacFormat {
            sample_rate: u32::try_from(setting_max_rate(setting)).unwrap_or(0),
            bits: u32::try_from(setting.bit_resolution).unwrap_or(0),
        })
        .filter(|f| f.sample_rate > 0 && f.bits > 0)
        .collect();
    UsbDacDetails {
        name: String::new(),
        maker: None,
        vendor_id: None,
        product_id: None,
        firmware: None,
        uac_version: uac_version(raw_descriptors),
        async_clock,
        max_power_ma: None,
        has_mic,
        rates: os_rates.to_vec(),
        bit_depths: os_bit_depths.to_vec(),
        bit_perfect,
        os_output,
    }
}

fn setting_max_rate(setting: &descriptors::Setting) -> i32 {
    setting
        .rates
        .iter()
        .map(|(_low, high)| *high)
        .max()
        .unwrap_or(0)
}

fn walk_descriptors(bytes: &[u8], mut visit: impl FnMut(u8, &[u8])) {
    let mut offset = 0usize;
    while offset + 2 <= bytes.len() {
        let length = bytes[offset] as usize;
        if length < 2 || offset + length > bytes.len() {
            break;
        }
        let kind = bytes[offset + 1];
        visit(kind, &bytes[offset..offset + length]);
        offset += length;
    }
}

/// UAC version from the first audio-control interface's protocol
/// (UsbDacDetails.kt:103-106: 0x20 → 2, 0x30 → 3, else 1).
fn uac_version(bytes: &[u8]) -> Option<u8> {
    let mut found = None;
    walk_descriptors(bytes, |kind, d| {
        if kind == 0x04 && d.len() >= 8 && d[5] == 1 && found.is_none() {
            found = match d[7] {
                0x20 => Some(2),
                0x30 => Some(3),
                _ => Some(1),
            };
        }
    });
    found
}

/// OUT isochronous endpoint attributes (the data direction our driver
/// streams to).
fn out_isochronous_endpoints(bytes: &[u8]) -> Vec<u8> {
    let mut found = Vec::new();
    walk_descriptors(bytes, |kind, d| {
        // bEndpointAddress & 0x80 == 0 (OUT), attributes & 3 == 1 (iso).
        if kind == 0x05 && d.len() >= 6 && d[2] & 0x80 == 0 && d[3] & 3 == 1 {
            found.push(d[3]);
        }
    });
    found
}

/// An IN iso endpoint carrying data (not the feedback type) — the mic.
fn has_microphone(bytes: &[u8]) -> bool {
    let mut found = false;
    walk_descriptors(bytes, |kind, d| {
        if kind == 0x05 && d.len() >= 6 && d[2] & 0x80 != 0 && d[3] & 3 == 1 && (d[3] >> 4) & 3 == 0
        {
            found = true;
        }
    });
    found
}

/// The per-device volume memory — upstream's `remembered` map in
/// AudioPass ("the level picked last time comes back, but never louder
/// than the dac's own knob was left", keyed `"vendor:product"`).
///
/// A tiny line format (`vid:pid=db`) keeps it dependency-free; the app
/// persists the string wherever settings live.
#[derive(Debug, Clone, Default)]
pub struct UsbVolumeMemory {
    remembered: std::collections::HashMap<(u16, u16), f64>,
}

impl UsbVolumeMemory {
    /// The remembered level for a device (never above `cap` — the DAC's
    /// own knob when we found it).
    pub fn level_for(&self, vendor: u16, product: u16, cap: f64) -> Option<f64> {
        self.remembered
            .get(&(vendor, product))
            .map(|db| db.min(cap))
    }

    /// Records the level that was current when the DAC was handed back.
    pub fn remember(&mut self, vendor: u16, product: u16, db: f64) {
        self.remembered.insert((vendor, product), db);
    }

    /// Serializes (the hand-back to persistence).
    pub fn to_lines(&self) -> String {
        let mut lines = Vec::new();
        for ((vendor, product), db) in &self.remembered {
            lines.push(format!("{vendor:04x}:{product:04x}={db}"));
        }
        lines.join("\n")
    }

    /// Deserializes; bad lines are dropped (a cache, not a contract).
    pub fn from_lines(text: &str) -> UsbVolumeMemory {
        let mut remembered = std::collections::HashMap::new();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let Some((vendor, product)) = key.split_once(':') else {
                continue;
            };
            if let (Ok(vendor), Ok(product), Ok(db)) = (
                u16::from_str_radix(vendor, 16),
                u16::from_str_radix(product, 16),
                value.parse(),
            ) {
                remembered.insert((vendor, product), db);
            }
        }
        UsbVolumeMemory { remembered }
    }
}

/// Reads the details of every attached USB audio device through rusb —
/// the `UsbDacReader.read` desktop half. Descriptors are read without
/// claiming (upstream read them through UsbManager the same way); the
/// OS audio side (`os_rates`/`os_bit_depths`/`os_output`) comes from
/// the caller, who owns the OS device list.
#[cfg(all(feature = "usb-uac", any(windows, target_os = "linux")))]
pub fn read_rusb(os_side: impl Fn(u16, u16) -> (Vec<u32>, Vec<u32>, bool)) -> Vec<UsbDacDetails> {
    let mut found = Vec::new();
    let Ok(devices) = rusb::DeviceList::new() else {
        return found;
    };
    for device in devices.iter() {
        let Ok(descriptor) = device.device_descriptor() else {
            continue;
        };
        if descriptor.class_code() != 0 && descriptor.class_code() != 0xEF {
            // Audio devices are interface-classed; 0xEF is the misc/IAD
            // class some composite DACs use.
        }
        let Ok(config) = device.active_config_descriptor() else {
            continue;
        };
        // A USB audio streaming interface means this is a DAC (the
        // UsbDacReader.streamingInterfaces filter).
        let has_audio = config.interfaces().any(|iface| {
            iface
                .descriptors()
                .any(|d| d.class_code() == 1 && d.sub_class_code() == 2)
        });
        if !has_audio {
            continue;
        }
        let Ok(handle) = device.open() else {
            continue;
        };
        // The raw descriptor blob UsbManager handed upstream: the device
        // descriptor followed by the active configuration's whole tree
        // (interfaces, endpoints, class descriptors). rusb parses instead
        // of handing bytes over, so both come from the standard
        // GET_DESCRIPTOR control request.
        let mut raw = get_descriptor(&handle, 0x01, 0, 18);
        raw.extend(get_descriptor(&handle, 0x02, config.number(), 0));
        let (rates, bit_depths, os_output) =
            os_side(descriptor.vendor_id(), descriptor.product_id());
        let mut details = read_from_descriptors(&raw, &rates, &bit_depths, os_output);
        details.vendor_id = Some(descriptor.vendor_id());
        details.product_id = Some(descriptor.product_id());
        if let Ok(languages) = handle.read_languages(std::time::Duration::from_secs(1))
            && let Some(language) = languages.first()
        {
            let timeout = std::time::Duration::from_secs(1);
            details.maker = handle
                .read_manufacturer_string(*language, &descriptor, timeout)
                .ok()
                .filter(|s| !s.trim().is_empty());
            details.name = handle
                .read_product_string(*language, &descriptor, timeout)
                .ok()
                .and_then(|s| {
                    let trimmed = s.trim().to_owned();
                    (!trimmed.is_empty()).then_some(trimmed)
                })
                .unwrap_or_else(|| "USB DAC".into());
            details.firmware = handle
                .read_serial_number_string(*language, &descriptor, timeout)
                .ok()
                .filter(|s| !s.trim().is_empty());
        }
        if details.name.is_empty() {
            details.name = "USB DAC".into();
        }
        details.max_power_ma = Some(config.max_power());
        found.push(details);
    }
    found
}

/// GET_DESCRIPTOR — the standard request (bmRequestType 0x80, bRequest
/// 6). `length` is the expected size; the configuration is read twice,
/// once for its header (wTotalLength at bytes 2..4) and once whole.
#[cfg(all(feature = "usb-uac", any(windows, target_os = "linux")))]
fn get_descriptor(
    handle: &rusb::DeviceHandle<rusb::GlobalContext>,
    kind: u8,
    index: u8,
    length: usize,
) -> Vec<u8> {
    let read = |want: usize| -> Option<Vec<u8>> {
        let mut buffer = vec![0u8; want];
        let moved = handle
            .read_control(
                0x80,
                0x06,
                (u16::from(kind) << 8) | u16::from(index),
                0,
                &mut buffer,
                std::time::Duration::from_secs(1),
            )
            .ok()?;
        buffer.truncate(moved);
        Some(buffer)
    };
    if kind == 0x02 {
        // The configuration's true length rides in its own header.
        let Some(head) = read(9) else {
            return Vec::new();
        };
        if head.len() < 4 {
            return Vec::new();
        }
        let total = u16::from_le_bytes([head[2], head[3]]) as usize;
        if total > head.len() {
            return read(total).unwrap_or_default();
        }
        return head;
    }
    read(length).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor(kind: u8, payload: &[u8]) -> Vec<u8> {
        let mut out = vec![u8::try_from(payload.len() + 2).unwrap(), kind];
        out.extend_from_slice(payload);
        out
    }

    fn dac_descriptors() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend(descriptor(
            0x01,
            &[0x00, 0x02, 0x02, 0x00, 0x00, 0x00, 0x40],
        ));
        bytes.extend(descriptor(0x04, &[0, 0, 1, 1, 0x01, 0x00, 0x00]));
        bytes.extend(descriptor(0x24, &[0x01, 0x01, 0x01, 0x2C, 0x00, 0x01]));
        bytes.extend(descriptor(
            0x24,
            &[0x02, 0x01, 0x01, 0x01, 0x00, 0x01, 0x00, 0x02],
        ));
        bytes.extend(descriptor(
            0x24,
            &[0x06, 0x02, 0x01, 0x01, 0x03, 0x00, 0x00, 0x00],
        ));
        bytes.extend(descriptor(
            0x24,
            &[0x03, 0x03, 0x01, 0x03, 0x00, 0x02, 0x00, 0x00],
        ));
        // The streaming interface's alt 0 and alt 1 follow their
        // control block — class descriptors belong to the interface
        // that precedes them.
        bytes.extend(descriptor(0x04, &[1, 0, 0, 1, 0x02, 0x00, 0x00]));
        bytes.extend(descriptor(0x04, &[1, 1, 2, 1, 0x02, 0x00, 0x00]));
        bytes.extend(descriptor(
            0x24,
            &[0x01, 0x01, 0x01, 0x01, 0x00, 0x02, 0x10, 0x01],
        ));
        let mut type1 = vec![0x02, 0x01, 0x02, 0x02, 0x10, 0x00];
        type1.extend_from_slice(&44_100i32.to_le_bytes()[..3]);
        type1.extend_from_slice(&48_000i32.to_le_bytes()[..3]);
        bytes.extend(descriptor(0x24, &type1));
        bytes.extend(descriptor(
            0x05,
            &[0x01, 0x05, 0xC4, 0x00, 0x01, 0x00, 0x00],
        ));
        bytes.extend(descriptor(0x25, &[0x01, 0x01]));
        bytes.extend(descriptor(
            0x05,
            &[0x81, 0x11, 0x04, 0x00, 0x01, 0x01, 0x00],
        ));
        bytes
    }

    #[test]
    fn derives_the_dac_facts() {
        let details = read_from_descriptors(&dac_descriptors(), &[48_000, 44_100], &[16], true);
        assert_eq!(details.uac_version, Some(1));
        assert_eq!(details.async_clock, Some(true), "the OUT endpoint is async");
        assert!(!details.has_mic);
        assert_eq!(details.rates, vec![48_000, 44_100]);
        assert_eq!(details.bit_depths, vec![16]);
        assert!(details.os_output);
        // The descriptor side's rates/depths ride bit_perfect.
        assert!(details.bit_perfect.contains(&DacFormat {
            sample_rate: 48_000,
            bits: 16
        }));
    }

    #[test]
    fn mic_endpoint_is_detected() {
        let mut bytes = dac_descriptors();
        // An IN iso endpoint of the data (not feedback) type = a mic.
        bytes.extend(descriptor(
            0x05,
            &[0x82, 0x01, 0x18, 0x00, 0x01, 0x00, 0x00],
        ));
        let details = read_from_descriptors(&bytes, &[], &[], false);
        assert!(details.has_mic);
    }

    #[test]
    fn volume_memory_round_trips_and_caps() {
        let mut memory = UsbVolumeMemory::default();
        memory.remember(0x20b1, 0x3008, -18.5);
        memory.remember(0x26a2, 0x1004, -6.0);
        let text = memory.to_lines();
        let back = UsbVolumeMemory::from_lines(&text);
        assert_eq!(back.level_for(0x20b1, 0x3008, 0.0), Some(-18.5));
        assert_eq!(back.level_for(0x26a2, 0x1004, 0.0), Some(-6.0));
        // Never louder than the cap (the DAC's knob when found).
        assert_eq!(back.level_for(0x20b1, 0x3008, -25.0), Some(-25.0));
        assert_eq!(back.level_for(0x9999, 0x9999, 0.0), None);
        // Bad lines drop.
        let scrappy = UsbVolumeMemory::from_lines("nope\n20b1:3008=xyz\n20b1:3008=-1.5\n");
        assert_eq!(scrappy.level_for(0x20b1, 0x3008, 0.0), Some(-1.5));
    }
}
