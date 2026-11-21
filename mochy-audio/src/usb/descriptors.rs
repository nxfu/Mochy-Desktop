//! USB Audio Class descriptor parsing — `UsbAudioDescriptors.kt`
//! ported 1:1.
//!
//! Reads a UAC 1 or 2 DAC's raw descriptors: which alternate setting,
//! endpoint and clock carry a song, and where its volume knob is. Pure
//! byte-slicing, always compiled (the `usb-uac` driver and the device
//! sheet both consume it; tests synthesize descriptors).
//!
//! Upstream: `playback/audio/pass/UsbAudioDescriptors.kt` (pin
//! `bde01cb4`, whole file, 269 lines). Structure, field order and the
//! plan-ranking rules (narrowest container first, async-endpoint
//! feedback requirement, UAC1 rate-list matching) are preserved
//! exactly; the `Plan` encoding is a struct instead of the 18-int
//! array the JNI boundary used.

/// One way the DAC can be streamed to — everything the driver needs to
/// set it up (`UsbAudioDescriptors.Plan`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub control_interface: i32,
    pub streaming_interface: i32,
    pub alt: i32,
    pub uac2: bool,
    /// The clock to set the rate on (UAC2), and the selector in front
    /// of it with the pin that leads there, or 0.
    pub clock_id: i32,
    pub selector_id: i32,
    pub selector_pin: i32,
    pub endpoint: i32,
    pub endpoint_attributes: i32,
    pub max_packet: i32,
    pub interval: i32,
    /// UAC1: the endpoint takes a sampling rate request.
    pub rate_control: bool,
    /// The endpoint the DAC reports its own pace on, or -1.
    pub feedback_endpoint: i32,
    pub feedback_max_packet: i32,
    pub rate: i32,
    pub channels: i32,
    /// Bytes per sample on the wire, and how many of those bits are
    /// real.
    pub subslot: i32,
    pub bit_resolution: i32,
}

/// The DAC's own volume control on the path from the stream to its
/// output (`VolumeUnit`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VolumeUnit {
    pub control_interface: i32,
    pub unit_id: i32,
    pub uac2: bool,
    pub channels: Vec<i32>,
}

/// The parsed DAC (`Dac`).
#[derive(Debug, Clone, Default)]
pub struct Dac {
    pub bcd_usb: i32,
    pub settings: Vec<Setting>,
    pub volume: Option<VolumeUnit>,
}

/// One alternate setting of a streaming interface that carries PCM out
/// to the DAC (`Setting`).
#[derive(Debug, Clone, Default)]
pub struct Setting {
    pub control_interface: i32,
    pub streaming_interface: i32,
    pub alt: i32,
    pub uac2: bool,
    pub terminal_link: i32,
    pub channels: i32,
    pub subslot: i32,
    pub bit_resolution: i32,
    /// UAC1 lists its rates; each pair is a range, a single rate is
    /// low == high.
    pub rates: Vec<(i32, i32)>,
    pub endpoint: Option<Endpoint>,
    pub feedback: Option<Endpoint>,
    /// UAC2: the clock that drives it, and the selector in front of
    /// that clock with the pin leading to it, or 0.
    pub clock_id: i32,
    pub selector_id: i32,
    pub selector_pin: i32,
}

/// An endpoint of a streaming alt setting (`Endpoint`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Endpoint {
    pub address: i32,
    pub attributes: i32,
    pub max_packet: i32,
    pub interval: i32,
    pub sync_address: i32,
    pub rate_control: bool,
}

#[derive(Debug, Clone)]
struct Entity {
    id: i32,
    subtype: i32,
    source: Vec<i32>,
    clock: i32,
    terminal_type: i32,
    volume_channels: Vec<i32>,
}

/// Parses a DAC out of its raw descriptor bytes (`parse`).
pub fn parse(bytes: &[u8]) -> Dac {
    let mut bcd_usb = 0;
    let mut iface = -1;
    // Assigned by every interface descriptor before any read; the
    // placeholder would only hide a descriptor stream that starts
    // mid-configuration.
    let mut alt;
    let mut iface_class = 0;
    let mut iface_sub = 0;
    let mut protocol = 0;
    let mut control = -1;
    let mut entities = std::collections::HashMap::new();
    let mut settings = Vec::new();
    let mut current: Option<Setting> = None;
    let mut endpoints: Vec<Endpoint> = Vec::new();

    let mut offset = 0usize;
    while offset + 2 <= bytes.len() {
        let length = bytes[offset] as usize;
        if length < 2 || offset + length > bytes.len() {
            break;
        }
        let d = &bytes[offset..offset + length];
        offset += length;
        let u = |i: usize| -> i32 { if i < d.len() { i32::from(d[i]) } else { 0 } };
        let u16 = |i: usize| -> i32 { u(i) | (u(i + 1) << 8) };
        let u24 = |i: usize| -> i32 { u16(i) | (u(i + 2) << 16) };
        match u(1) {
            0x01 => bcd_usb = u16(2),
            0x04 => {
                finish_setting(&mut current, &mut endpoints, &mut settings);
                iface = u(2);
                alt = u(3);
                iface_class = u(5);
                iface_sub = u(6);
                protocol = u(7);
                if iface_class == 1 && iface_sub == 1 {
                    control = iface;
                }
                if iface_class == 1 && iface_sub == 2 && alt > 0 && control >= 0 {
                    current = Some(Setting {
                        control_interface: control,
                        streaming_interface: iface,
                        alt,
                        uac2: protocol == 0x20,
                        terminal_link: 0,
                        channels: 0,
                        subslot: 0,
                        bit_resolution: 0,
                        rates: Vec::new(),
                        endpoint: None,
                        feedback: None,
                        clock_id: 0,
                        selector_id: 0,
                        selector_pin: 0,
                    });
                }
            }
            0x05 => {
                if current.is_some() && length >= 7 {
                    let packet = u16(4);
                    endpoints.push(Endpoint {
                        address: u(2),
                        attributes: u(3),
                        // High-bandwidth endpoints carry up to three
                        // transactions a microframe.
                        max_packet: (packet & 0x7ff) * (1 + ((packet >> 11) & 3)),
                        interval: u(6),
                        sync_address: if length >= 9 { u(8) } else { 0 },
                        rate_control: false,
                    });
                }
            }
            0x25 => {
                if current.is_some() && length >= 4 && u(2) == 1 && !endpoints.is_empty() {
                    let last = endpoints.last_mut().expect("checked non-empty");
                    last.rate_control = u(3) & 1 != 0;
                }
            }
            0x24 => {
                if iface_class == 1 && iface_sub == 1 {
                    if let Some(entity) = control_entity(d, protocol == 0x20) {
                        entities.insert((iface, entity.id), entity);
                    }
                } else if let Some(setting) = current.as_mut() {
                    streaming_entity(setting, d, &u, &u24);
                }
            }
            _ => {}
        }
    }
    finish_setting(&mut current, &mut endpoints, &mut settings);
    let pcm: Vec<Setting> = settings
        .into_iter()
        .filter(|s| {
            s.endpoint.is_some()
                && (1..=2).contains(&s.channels)
                && (2..=4).contains(&s.subslot)
                && (16..=32).contains(&s.bit_resolution)
        })
        .map(|s| if s.uac2 { with_clock(s, &entities) } else { s })
        .collect();
    let volume = volume_unit(&pcm, &entities);
    Dac {
        bcd_usb,
        settings: pcm,
        volume,
    }
}

/// Closes out a streaming alt setting: picks the data endpoint (OUT,
/// iso) and the feedback endpoint (IN, iso, async-typed or the data
/// endpoint's sync address) — the `finish` closure.
fn finish_setting(
    current: &mut Option<Setting>,
    endpoints: &mut Vec<Endpoint>,
    settings: &mut Vec<Setting>,
) {
    let Some(setting) = current.take() else {
        return;
    };
    let data = endpoints
        .iter()
        .find(|ep| ep.address & 0x80 == 0 && ep.attributes & 3 == 1)
        .copied();
    let feedback = endpoints
        .iter()
        .find(|ep| {
            ep.address & 0x80 != 0
                && ep.attributes & 3 == 1
                && (((ep.attributes >> 4) & 3 == 1)
                    || data.is_some_and(|d| ep.address == d.sync_address))
        })
        .copied();
    settings.push(Setting {
        endpoint: data,
        feedback,
        ..setting
    });
    endpoints.clear();
}

/// A control-interface entity (input/output terminal, mixer, selector,
/// feature unit, clock) — `controlEntity`.
fn control_entity(d: &[u8], uac2: bool) -> Option<Entity> {
    let u = |i: usize| -> i32 { if i < d.len() { i32::from(d[i]) } else { 0 } };
    let u16 = |i: usize| -> i32 { u(i) | (u(i + 1) << 8) };
    let subtype = u(2);
    let entity = match subtype {
        // Input terminal: on UAC2 it names the clock that drives it.
        0x02 => Entity {
            id: u(3),
            subtype,
            source: Vec::new(),
            clock: if uac2 { u(7) } else { 0 },
            terminal_type: u16(4),
            volume_channels: Vec::new(),
        },
        0x03 => Entity {
            id: u(3),
            subtype,
            source: vec![u(7)],
            clock: if uac2 { u(8) } else { 0 },
            terminal_type: u16(4),
            volume_channels: Vec::new(),
        },
        0x04 | 0x05 => Entity {
            id: u(3),
            subtype,
            source: (0..u(4)).map(|i| u(5 + i as usize)).collect(),
            clock: 0,
            terminal_type: 0,
            volume_channels: Vec::new(),
        },
        0x06 => Entity {
            id: u(3),
            subtype,
            source: vec![u(4)],
            clock: 0,
            terminal_type: 0,
            volume_channels: feature_volume(d, uac2),
        },
        // Clock source, clock selector and multiplier only exist on
        // UAC2.
        0x0a => uac2.then(|| Entity {
            id: u(3),
            subtype,
            source: Vec::new(),
            clock: 0,
            terminal_type: 0,
            volume_channels: Vec::new(),
        })?,
        0x0b => uac2.then(|| Entity {
            id: u(3),
            subtype,
            source: (0..u(4)).map(|i| u(5 + i as usize)).collect(),
            clock: 0,
            terminal_type: 0,
            volume_channels: Vec::new(),
        })?,
        0x0c => uac2.then(|| Entity {
            id: u(3),
            subtype,
            source: vec![u(4)],
            clock: 0,
            terminal_type: 0,
            volume_channels: Vec::new(),
        })?,
        _ => return None,
    };
    Some(entity)
}

/// The channels whose volume the host can set: 0 is the master —
/// `featureVolume`.
fn feature_volume(d: &[u8], uac2: bool) -> Vec<i32> {
    let u = |i: usize| -> i32 { if i < d.len() { i32::from(d[i]) } else { 0 } };
    let size = if uac2 { 4 } else { u(5) } as usize;
    let start = if uac2 { 5 } else { 6 } as usize;
    if size == 0 {
        return Vec::new();
    }
    if d.len() < start + 1 {
        return Vec::new();
    }
    let count = (d.len() - start - 1) / size;
    (0..count)
        .filter(|ch| {
            let first = u(start + ch * size);
            if uac2 {
                (first >> 2) & 3 == 3
            } else {
                first & 0x02 != 0
            }
        })
        .map(|ch| ch as i32)
        .collect()
}

/// A class-specific streaming descriptor — `streamingEntity` (the
/// general/CSD/AS descriptors for terminal link, channels, subslot,
/// bits, rates).
fn streaming_entity(
    s: &mut Setting,
    _d: &[u8],
    u: &dyn Fn(usize) -> i32,
    u24: &dyn Fn(usize) -> i32,
) {
    match (u(2), u(3)) {
        (1, _) if s.uac2 => {
            if u(5) == 1 && (u(6) & 1) != 0 {
                s.terminal_link = u(3);
                s.channels = u(10);
            } else {
                s.terminal_link = u(3);
                s.channels = 0;
            }
        }
        (1, _) => {
            if (u(5) | (u(6) << 8)) == 1 {
                s.terminal_link = u(3);
            } else {
                s.terminal_link = u(3);
                s.subslot = -1;
            }
        }
        (2, 1) if s.uac2 => {
            s.subslot = u(4);
            s.bit_resolution = u(5);
        }
        (2, 1) if s.subslot >= 0 => {
            let count = u(7);
            let rates = if count == 0 {
                vec![(u24(8), u24(11))]
            } else {
                (0..count)
                    .map(|i| {
                        let r = u24((8 + i * 3) as usize);
                        (r, r)
                    })
                    .collect()
            };
            s.channels = u(4);
            s.subslot = u(5);
            s.bit_resolution = u(6);
            s.rates = rates;
        }
        _ => {}
    }
}

/// Follows a setting's terminal to its clock; a selector with several
/// inputs is pointed at the first clock source behind it —
/// `withClock`.
fn with_clock(s: Setting, entities: &std::collections::HashMap<(i32, i32), Entity>) -> Setting {
    let mut id = entities
        .get(&(s.control_interface, s.terminal_link))
        .map(|entity| entity.clock)
        .unwrap_or(0);
    let mut selector = 0;
    let mut pin = 0;
    for _ in 0..4 {
        let Some(entity) = entities.get(&(s.control_interface, id)) else {
            break;
        };
        match entity.subtype {
            0x0b => {
                let index = entity
                    .source
                    .iter()
                    .position(|src| {
                        entities
                            .get(&(s.control_interface, *src))
                            .is_some_and(|e| e.subtype == 0x0a)
                    })
                    .unwrap_or(0);
                selector = entity.id;
                pin = index as i32 + 1;
                id = entity.source.get(index).copied().unwrap_or(0);
            }
            0x0c => {
                id = entity.source.first().copied().unwrap_or(0);
            }
            _ => break,
        }
    }
    let is_source = entities
        .get(&(s.control_interface, id))
        .is_some_and(|e| e.subtype == 0x0a);
    Setting {
        clock_id: if is_source { id } else { 0 },
        selector_id: selector,
        selector_pin: pin,
        ..s
    }
}

/// The one feature unit with a settable volume between the stream's
/// terminal and an output; anything ambiguous means no volume —
/// `volumeUnit`.
fn volume_unit(
    settings: &[Setting],
    entities: &std::collections::HashMap<(i32, i32), Entity>,
) -> Option<VolumeUnit> {
    let setting = settings.first()?;
    let control = setting.control_interface;
    let outputs = entities
        .values()
        .filter(|e| e.subtype == 0x03 && e.terminal_type != 0x0101)
        .filter(|e| {
            entities
                .get(&(control, e.id))
                .is_some_and(|found| std::ptr::eq(found, *e))
        });
    let mut found = std::collections::HashSet::new();
    for output in outputs {
        let mut id = output.source.first().copied()?;
        let mut unit: Option<VolumeUnit> = None;
        let mut seen = std::collections::HashSet::new();
        while id != setting.terminal_link && seen.insert(id) {
            let Some(entity) = entities.get(&(control, id)) else {
                break;
            };
            if unit.is_none() && entity.subtype == 0x06 && !entity.volume_channels.is_empty() {
                let channels = if entity.volume_channels.contains(&0) {
                    vec![0]
                } else {
                    entity
                        .volume_channels
                        .iter()
                        .copied()
                        .filter(|ch| (1..=setting.channels).contains(ch))
                        .collect()
                };
                unit = Some(VolumeUnit {
                    control_interface: control,
                    unit_id: entity.id,
                    uac2: setting.uac2,
                    channels,
                });
            }
            // A mixer or selector could route from somewhere else:
            // guessing could turn the wrong thing down.
            if entity.source.len() != 1 {
                break;
            }
            id = entity.source[0];
        }
        if id == setting.terminal_link
            && let Some(unit) = unit
        {
            found.insert(unit);
        }
    }
    if found.len() == 1 {
        found.into_iter().next()
    } else {
        None
    }
}

/// Every way this DAC can carry a song of this rate, channels and
/// depth, best first: the narrowest container that holds it whole —
/// `plans`.
pub fn plans(dac: &Dac, rate: i32, channels: i32, source_bits: i32) -> Vec<Plan> {
    let mut candidates: Vec<&Setting> = dac
        .settings
        .iter()
        .filter(|s| {
            s.channels == channels
                && s.bit_resolution >= source_bits
                && s.subslot * 8 >= s.bit_resolution
        })
        .filter(|s| {
            s.uac2
                || s.rates
                    .iter()
                    .any(|(low, high)| (*low..=*high).contains(&rate))
        })
        .collect();
    candidates.sort_by_key(|s| (s.subslot, s.bit_resolution));
    candidates
        .into_iter()
        .filter_map(|s| plan_for(s, rate))
        .collect()
}

/// One setting → one plan, or none when it can't carry the song —
/// `planFor`.
fn plan_for(s: &Setting, rate: i32) -> Option<Plan> {
    let data = s.endpoint?;
    if !(1..=4).contains(&data.interval) || data.max_packet == 0 {
        return None;
    }
    // An async endpoint with no feedback endpoint paces itself off the
    // DAC's mic stream, which we don't run.
    if (data.attributes >> 2) & 3 == 1 && s.feedback.is_none() {
        return None;
    }
    // UAC1 with no rate request only plays the one rate it lists.
    if !s.uac2 && !data.rate_control && s.rates != vec![(rate, rate)] {
        return None;
    }
    if s.uac2 && s.clock_id == 0 {
        return None;
    }
    Some(Plan {
        control_interface: s.control_interface,
        streaming_interface: s.streaming_interface,
        alt: s.alt,
        uac2: s.uac2,
        clock_id: s.clock_id,
        selector_id: s.selector_id,
        selector_pin: s.selector_pin,
        endpoint: data.address,
        endpoint_attributes: data.attributes,
        max_packet: data.max_packet,
        interval: data.interval,
        rate_control: data.rate_control,
        feedback_endpoint: s.feedback.map(|ep| ep.address).unwrap_or(-1),
        feedback_max_packet: s.feedback.map(|ep| ep.max_packet).unwrap_or(0),
        rate,
        channels: s.channels,
        subslot: s.subslot,
        bit_resolution: s.bit_resolution,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor(kind: u8, payload: &[u8]) -> Vec<u8> {
        let mut out = vec![u8::try_from(payload.len() + 2).unwrap(), kind];
        out.extend_from_slice(payload);
        out
    }

    /// A UAC1 DAC: control interface (AC header + feature unit + output
    /// terminal), streaming interface alt 1 (AS general + Type I + iso
    /// OUT endpoint + rate control).
    fn uac1_descriptors(rate_low: i32, rate_high: i32, rate_control: bool) -> Vec<u8> {
        let mut bytes = Vec::new();
        // device descriptor (bcdUSB)
        bytes.extend(descriptor(
            0x01,
            &[0x00, 0x02, 0x02, 0x00, 0x00, 0x00, 0x40],
        ));
        // config + interface association skipped; control interface #0
        bytes.extend(descriptor(
            0x04,
            &[0, 0, 1, 1, 0x01, 0x00, 0x00], // iface 0 alt 0 class 1 sub 1 protocol 0
        ));
        // AC header (class-specific, 0x24 subtype 1)
        bytes.extend(descriptor(0x24, &[0x01, 0x00, 0x01, 0x2C, 0x00, 0x01]));
        // Input terminal (id 1, type 0x0101 USB streaming), UAC1
        bytes.extend(descriptor(
            0x24,
            &[0x02, 0x01, 0x01, 0x01, 0x00, 0x01, 0x00, 0x02],
        ));
        // Feature unit (id 2, source 1, control size 1, ch0+ch1 volume)
        bytes.extend(descriptor(
            0x24,
            &[0x06, 0x02, 0x01, 0x01, 0x03, 0x00, 0x00, 0x00],
        ));
        // Output terminal (id 3, type 0x0301 speaker, source 2)
        bytes.extend(descriptor(
            0x24,
            &[0x03, 0x03, 0x01, 0x03, 0x00, 0x02, 0x00, 0x00],
        ));
        // Streaming interface #1 alt 0 then alt 1
        bytes.extend(descriptor(0x04, &[1, 0, 0, 1, 0x02, 0x00, 0x00]));
        bytes.extend(descriptor(0x04, &[1, 1, 2, 1, 0x02, 0x00, 0x00]));
        // AS general (subtype 1, terminal link 1, delay 1, format PCM)
        bytes.extend(descriptor(
            0x24,
            &[0x01, 0x01, 0x01, 0x01, 0x00, 0x02, 0x10, 0x01],
        ));
        // Type I format: channels 2, subslot 2, bits 16, one rate range
        let mut type1 = vec![0x02, 0x01, 0x02, 0x02, 0x10, 0x00];
        type1.extend_from_slice(&rate_low.to_le_bytes()[..3]);
        type1.extend_from_slice(&rate_high.to_le_bytes()[..3]);
        bytes.extend(descriptor(0x24, &type1));
        // Iso OUT endpoint: address 0x01, attributes 0x05 (iso + async),
        // max packet 196, interval 1
        bytes.extend(descriptor(
            0x05,
            &[0x01, 0x05, 0xC4, 0x00, 0x01, 0x00, 0x00],
        ));
        // Endpoint rate control (0x25 subtype 1)
        bytes.extend(descriptor(0x25, &[0x01, u8::from(rate_control)]));
        // Feedback IN endpoint: address 0x81, attributes 0x11 (iso + feedback? type 1? sync bits)
        bytes.extend(descriptor(
            0x05,
            &[0x81, 0x11, 0x04, 0x00, 0x01, 0x01, 0x00],
        ));
        bytes
    }

    #[test]
    fn parses_a_uac1_dac() {
        let dac = parse(&uac1_descriptors(44_100, 48_000, true));
        assert_eq!(dac.bcd_usb, 0x0200);
        assert_eq!(dac.settings.len(), 1);
        let setting = &dac.settings[0];
        assert_eq!(setting.channels, 2);
        assert_eq!(setting.subslot, 2);
        assert_eq!(setting.bit_resolution, 16);
        assert_eq!(setting.rates, vec![(44_100, 48_000)]);
        assert!(setting.endpoint.is_some());
        assert_eq!(setting.endpoint.unwrap().address, 0x01);
        assert!(setting.endpoint.unwrap().rate_control);
        assert!(setting.feedback.is_some(), "async endpoint needs feedback");
        // The feature unit between the streaming terminal and the
        // output.
        let volume = dac.volume.as_ref().unwrap();
        assert_eq!(volume.unit_id, 2);
        assert_eq!(volume.channels, vec![0]);
    }

    #[test]
    fn plans_rank_and_filter() {
        let dac = parse(&uac1_descriptors(44_100, 48_000, true));
        // A 16-bit stereo song at 44.1 kHz: one plan.
        let got = plans(&dac, 44_100, 2, 16);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].rate, 44_100);
        assert_eq!(got[0].endpoint, 0x01);
        assert!(got[0].rate_control);
        assert_eq!(got[0].feedback_endpoint, 0x81);
        // A rate the DAC doesn't list: no plan.
        assert!(plans(&dac, 96_000, 2, 16).is_empty());
        // More bits than the setting carries: no plan.
        assert!(plans(&dac, 44_100, 2, 24).is_empty());
    }

    #[test]
    fn no_rate_control_pins_to_single_rates() {
        // UAC1 without the rate request only plays exactly-listed rates.
        let dac = parse(&uac1_descriptors(48_000, 48_000, false));
        assert_eq!(plans(&dac, 48_000, 2, 16).len(), 1);
        assert!(plans(&dac, 44_100, 2, 16).is_empty());
    }

    #[test]
    fn async_without_feedback_is_refused() {
        let mut bytes = uac1_descriptors(44_100, 48_000, true);
        // Strip the feedback endpoint (the last 9 bytes).
        bytes.truncate(bytes.len() - 9);
        let dac = parse(&bytes);
        // Data endpoint is async (attrs 0x05) with no feedback: no plan.
        assert!(plans(&dac, 44_100, 2, 16).is_empty());
    }

    #[test]
    fn volume_requires_an_unambiguous_path() {
        // Two feature units on two output terminals → the singleOrNull
        // rule drops the volume (one shared unit would still be one
        // answer). The new entities splice in before the streaming
        // interface, where the parser still reads control entities.
        let mut bytes = uac1_descriptors(44_100, 48_000, true);
        let mut extra = Vec::new();
        // A second feature unit (id 5, also from the input terminal).
        extra.extend(descriptor(
            0x24,
            &[0x06, 0x05, 0x01, 0x01, 0x03, 0x00, 0x00, 0x00],
        ));
        // A second output terminal (id 4, type headset, source 5).
        extra.extend(descriptor(
            0x24,
            &[0x03, 0x04, 0x02, 0x04, 0x00, 0x05, 0x00, 0x00],
        ));
        // The streaming interface's first descriptor: iface 1, alt 0.
        let streaming = [0x09, 0x04, 0x01, 0x00, 0x00, 0x01, 0x02, 0x00, 0x00];
        let at = bytes
            .windows(streaming.len())
            .position(|window| window == streaming)
            .expect("the streaming interface is in the fixture");
        bytes.splice(at..at, extra);
        let dac = parse(&bytes);
        assert!(dac.volume.is_none());
    }
}
