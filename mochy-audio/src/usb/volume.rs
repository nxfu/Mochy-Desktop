//! The DAC's own attenuator — `UsbDacVolume.kt` ported 1:1.
//!
//! "the dac's own attenuator, set in 1/256 dB steps: how the direct
//! driver has a volume without scaling a single sample." Everything is
//! driven through the [`UsbTransfer`] trait (a control-pipe request:
//! type, request, value, index, bytes → how many moved or a negative
//! error), so the volume logic compiles and tests without hardware;
//! the `usb-uac` driver wires the trait onto rusb's control transfer.

/// A request on the DAC's control pipe (`UsbTransfer`).
pub trait UsbTransfer {
    fn transfer(
        &mut self,
        request_type: i32,
        request: i32,
        value: i32,
        index: i32,
        data: &mut [u8],
    ) -> i32;
}

/// The DAC's volume: ranges, steps, and each channel's level from
/// before we touched it (`UsbDacVolume`).
#[derive(Debug)]
pub struct UsbDacVolume {
    unit: crate::usb::descriptors::VolumeUnit,
    ranges: Vec<std::ops::RangeInclusive<i32>>,
    steps: Vec<i32>,
    original: Vec<i32>,
    /// The current level, dB (1/256 steps ÷ 256).
    pub min_db: f64,
    pub max_db: f64,
    current_db: std::cell::Cell<f64>,
}

const VOLUME_CONTROL: i32 = 0x02;
const SET_CUR: i32 = 0x01;
const UAC2_CUR: i32 = 0x01;
const UAC2_RANGE: i32 = 0x02;
const UAC1_GET_CUR: i32 = 0x81;
const UAC1_GET_MIN: i32 = 0x82;
const UAC1_GET_MAX: i32 = 0x83;
const UAC1_GET_RES: i32 = 0x84;

/// The level the DAC is brought down to before the first sound, unless
/// it was already quieter (`StartDb = -40`).
pub const START_DB: f64 = -40.0;
/// Below this the DAC's range is all silence anyway, so the bar's
/// travel starts here (`FloorDb = -80`, AudioPass.kt:299).
pub const FLOOR_DB: f64 = -80.0;
/// One press of a volume key: 16 presses from silent to full
/// (`VolumeSteps = 16`, AudioPass.kt:287).
pub const VOLUME_STEPS: i32 = 16;

impl UsbDacVolume {
    /// The quietest channel as we found it, so a remembered level never
    /// comes back louder than the DAC's knob allows (`originalDb`).
    pub fn original_db(&self) -> f64 {
        self.original.iter().min().copied().unwrap_or(0) as f64 / 256.0
    }

    /// The current level in dB.
    pub fn current_db(&self) -> f64 {
        self.current_db.get()
    }

    /// Sets every channel to `db` (never above 0) and returns what the
    /// DAC really settled on (`set`).
    pub fn set(&mut self, transfer: &mut dyn UsbTransfer, db: f64) -> f64 {
        let target = (db.clamp(self.min_db, self.max_db) * 256.0).floor() as i32;
        let targets = vec![target; self.unit.channels.len()];
        self.apply(transfer, &targets);
        self.current_db.get()
    }

    /// Puts every channel back where it was found (`restore`).
    pub fn restore(&mut self, transfer: &mut dyn UsbTransfer) {
        let original = self.original.clone();
        self.apply(transfer, &original);
    }

    fn index(&self) -> i32 {
        (self.unit.unit_id << 8) | self.unit.control_interface
    }

    fn apply(&mut self, transfer: &mut dyn UsbTransfer, targets: &[i32]) {
        let mut loudest = i16::MIN as i32;
        for (i, channel) in self.unit.channels.iter().enumerate() {
            let range = self.ranges[i].clone();
            let step = self.steps[i].max(1);
            // Rounded down to the DAC's own steps, so it never ends up
            // louder than asked.
            let value = if targets[i] <= *range.start() {
                *range.start()
            } else {
                *range.start() + (targets[i].min(*range.end()) - *range.start()) / step * step
            };
            let bytes = [(value & 0xFF) as u8, ((value >> 8) & 0xFF) as u8];
            transfer.transfer(
                0x21,
                SET_CUR,
                (VOLUME_CONTROL << 8) | channel,
                self.index(),
                &mut bytes.to_vec(),
            );
            let request = if self.unit.uac2 {
                UAC2_CUR
            } else {
                UAC1_GET_CUR
            };
            loudest =
                loudest.max(read_channel(transfer, &self.unit, request, *channel).unwrap_or(value));
        }
        self.current_db
            .set(f64::from(loudest.clamp(i16::MIN as i32, i16::MAX as i32)) / 256.0);
    }

    /// Reads the DAC's range and level, then turns it down to at most
    /// [`START_DB`] before anything plays; `None` when it has no usable
    /// volume (`open`).
    pub fn open(
        transfer: &mut dyn UsbTransfer,
        unit: crate::usb::descriptors::VolumeUnit,
    ) -> Option<UsbDacVolume> {
        if unit.channels.is_empty() {
            return None;
        }
        let mut ranges = Vec::new();
        let mut steps = Vec::new();
        let mut original = Vec::new();
        for channel in &unit.channels {
            let request = if unit.uac2 { UAC2_CUR } else { UAC1_GET_CUR };
            let current = read_channel(transfer, &unit, request, *channel)?;
            original.push(current);
            if unit.uac2 {
                let mut head = [0u8; 2];
                let index = (unit.unit_id << 8) | unit.control_interface;
                if transfer.transfer(
                    0xA1,
                    UAC2_RANGE,
                    (VOLUME_CONTROL << 8) | channel,
                    index,
                    &mut head,
                ) != 2
                {
                    return None;
                }
                let count = i32::from(head[0]) | (i32::from(head[1]) << 8);
                if !(1..=16).contains(&count) {
                    return None;
                }
                let mut all = vec![0u8; 2 + count as usize * 6];
                if transfer.transfer(
                    0xA1,
                    UAC2_RANGE,
                    (VOLUME_CONTROL << 8) | channel,
                    index,
                    &mut all,
                ) != all.len() as i32
                {
                    return None;
                }
                let s16 = |at: usize| i16::from_le_bytes([all[at], all[at + 1]]) as i32;
                // Several sub-ranges become one from the lowest to the
                // highest, with the finest step.
                ranges.push(s16(2)..=s16(2 + (count as usize - 1) * 6 + 2));
                steps.push(
                    (0..count as usize)
                        .map(|i| s16(2 + i * 6 + 4))
                        .min()
                        .unwrap_or(1),
                );
            } else {
                let min = read_channel(transfer, &unit, UAC1_GET_MIN, *channel)?;
                let max = read_channel(transfer, &unit, UAC1_GET_MAX, *channel)?;
                ranges.push(min..=max);
                steps.push(read_channel(transfer, &unit, UAC1_GET_RES, *channel).unwrap_or(1));
            }
        }
        let min_db = ranges.iter().map(|r| *r.start()).max()? as f64 / 256.0;
        let max_db = ranges.iter().map(|r| *r.end()).min()?.min(0) as f64 / 256.0;
        // -32768 means minus infinity on USB; a DAC whose knob can't
        // reach -40 dB can't be started quietly.
        if min_db >= max_db || min_db > START_DB {
            return None;
        }
        let mut volume = UsbDacVolume {
            unit,
            ranges,
            steps,
            original: original.clone(),
            min_db,
            max_db,
            current_db: std::cell::Cell::new(min_db),
        };
        volume.apply(
            transfer,
            &original
                .iter()
                .map(|v| (*v).min((START_DB * 256.0) as i32))
                .collect::<Vec<_>>(),
        );
        Some(volume)
    }
}

fn read_channel(
    transfer: &mut dyn UsbTransfer,
    unit: &crate::usb::descriptors::VolumeUnit,
    request: i32,
    channel: i32,
) -> Option<i32> {
    let mut data = [0u8; 2];
    let got = transfer.transfer(
        0xA1,
        request,
        (VOLUME_CONTROL << 8) | channel,
        (unit.unit_id << 8) | unit.control_interface,
        &mut data,
    );
    if got == 2 {
        Some(i16::from_le_bytes([data[0], data[1]]) as i32)
    } else {
        None
    }
}

/// The 0..1 level of a volume for UI mapping (`AudioPass.levelOf`):
/// the travel between the floor and the max.
pub fn level_of(min_db: f64, max_db: f64, current_db: f64) -> f32 {
    let floor = min_db.max(FLOOR_DB);
    if current_db <= floor || max_db <= floor {
        return 0.0;
    }
    (((current_db - floor) / (max_db - floor)) as f32).clamp(0.0, 1.0)
}

/// The dB for a 0..1 level (`AudioPass.dbAt`).
pub fn db_at(min_db: f64, max_db: f64, level: f32) -> f64 {
    if level <= 0.0 {
        return min_db;
    }
    let floor = min_db.max(FLOOR_DB);
    floor + f64::from(level) * (max_db - floor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usb::descriptors::VolumeUnit;
    use std::cell::RefCell;

    /// A mock control pipe remembering SET_CURs and answering GETs from
    /// a table (UAC1-shaped).
    struct MockPipe {
        min: i32,
        max: i32,
        res: i32,
        current: RefCell<i32>,
        writes: RefCell<Vec<(i32, i32)>>, // (channel, value)
    }

    impl UsbTransfer for MockPipe {
        fn transfer(
            &mut self,
            request_type: i32,
            request: i32,
            value: i32,
            _index: i32,
            data: &mut [u8],
        ) -> i32 {
            // wValue's low byte is the channel, the high byte the control
            // selector — the recorded channel is the low one.
            let channel = value & 0xFF;
            if request_type == 0x21 && request == SET_CUR {
                let value = i16::from_le_bytes([data[0], data[1]]) as i32;
                self.writes.borrow_mut().push((channel, value));
                *self.current.borrow_mut() = value;
                2
            } else if request_type == 0xA1 {
                let answer = match request {
                    UAC1_GET_CUR => *self.current.borrow(),
                    UAC1_GET_MIN => self.min,
                    UAC1_GET_MAX => self.max,
                    UAC1_GET_RES => self.res,
                    _ => return -1,
                };
                data[0] = (answer & 0xFF) as u8;
                data[1] = ((answer >> 8) & 0xFF) as u8;
                2
            } else {
                -1
            }
        }
    }

    fn unit(channels: Vec<i32>) -> VolumeUnit {
        VolumeUnit {
            control_interface: 0,
            unit_id: 2,
            uac2: false,
            channels,
        }
    }

    #[test]
    fn open_reads_the_range_and_lowers_to_start_db() {
        // A knob from -60 to 0 dB in 1 dB steps, found at -10 dB.
        let mut pipe = MockPipe {
            min: -60 * 256,
            max: 0,
            res: 256,
            current: RefCell::new(-10 * 256),
            writes: RefCell::new(Vec::new()),
        };
        let volume = UsbDacVolume::open(&mut pipe, unit(vec![0])).unwrap();
        assert_eq!(volume.min_db, -60.0);
        assert_eq!(volume.max_db, 0.0);
        // The pre-lower: found at -10, StartDb -40 → set to -40.
        assert_eq!(pipe.writes.borrow()[0], (0, -40 * 256));
        assert_eq!(volume.current_db(), -40.0);
        // The original level is remembered for restore.
        assert_eq!(volume.original_db(), -10.0);
    }

    #[test]
    fn set_rounds_down_to_steps_and_reports_back() {
        let mut pipe = MockPipe {
            min: -90 * 256,
            max: 0,
            res: 512, // 2 dB steps
            current: RefCell::new(-40 * 256),
            writes: RefCell::new(Vec::new()),
        };
        let mut volume = UsbDacVolume::open(&mut pipe, unit(vec![0])).unwrap();
        // Ask for -30.5 dB with 2 dB steps: lands on -30 (a multiple of
        // 2 from the min... -90 + steps of 2 → even offsets only).
        let settled = volume.set(&mut pipe, -30.5);
        // -30.5 rounds toward the DAC's step grid below it.
        assert!((-32.5..=-30.0).contains(&settled), "settled {settled}");
        assert_eq!(volume.current_db(), settled);
    }

    #[test]
    fn restore_puts_the_knob_back() {
        let mut pipe = MockPipe {
            min: -60 * 256,
            max: 0,
            res: 256,
            current: RefCell::new(-5 * 256),
            writes: RefCell::new(Vec::new()),
        };
        let mut volume = UsbDacVolume::open(&mut pipe, unit(vec![0])).unwrap();
        volume.set(&mut pipe, -20.0);
        volume.restore(&mut pipe);
        // The last write is the restore to the original -5 dB.
        let writes = pipe.writes.borrow();
        assert_eq!(*writes.last().unwrap(), (0, -5 * 256));
    }

    #[test]
    fn a_knob_that_cant_go_quiet_enough_is_refused() {
        let mut pipe = MockPipe {
            min: -20 * 256, // can't reach -40
            max: 0,
            res: 256,
            current: RefCell::new(-10 * 256),
            writes: RefCell::new(Vec::new()),
        };
        assert!(UsbDacVolume::open(&mut pipe, unit(vec![0])).is_none());
    }

    #[test]
    fn level_and_db_map_the_travel() {
        // The AudioPass mapping: floor at max(minDb, -80).
        assert_eq!(level_of(-90.0, 0.0, -90.0), 0.0);
        assert_eq!(level_of(-60.0, 0.0, -60.0), 0.0); // at floor
        assert!((level_of(-80.0, 0.0, -40.0) - 0.5).abs() < 1e-6);
        assert_eq!(level_of(-60.0, 0.0, 10.0), 1.0); // above max clamps
        assert_eq!(db_at(-60.0, 0.0, 0.0), -60.0); // level 0 → min
        assert!((db_at(-80.0, 0.0, 0.5) - -40.0).abs() < 1e-6);
        // Round trip.
        let level = level_of(-50.0, 0.0, -25.0);
        assert!((db_at(-50.0, 0.0, level) - -25.0).abs() < 1e-6);
    }

    #[test]
    fn volume_steps_cover_the_travel() {
        // 16 presses from silent to full.
        let mut level = 0.0f32;
        for _ in 0..VOLUME_STEPS {
            level = (level + 1.0 / VOLUME_STEPS as f32).min(1.0);
        }
        assert!((level - 1.0).abs() < 1e-6);
    }
}
