//! CMOS real-time clock reader for x86_64.

use spin::Mutex;

const CMOS_INDEX: u16 = 0x70;
const CMOS_DATA: u16 = 0x71;
const REG_SECONDS: u8 = 0x00;
const REG_MINUTES: u8 = 0x02;
const REG_HOURS: u8 = 0x04;
const REG_DAY: u8 = 0x07;
const REG_MONTH: u8 = 0x08;
const REG_YEAR: u8 = 0x09;
const REG_STATUS_A: u8 = 0x0A;
const REG_STATUS_B: u8 = 0x0B;
const UPDATE_IN_PROGRESS: u8 = 1 << 7;
const BINARY_MODE: u8 = 1 << 2;
const TWENTY_FOUR_HOUR_MODE: u8 = 1 << 1;
const PM_FLAG: u8 = 1 << 7;
const MAX_READ_ATTEMPTS: usize = 100_000;

static CMOS_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RtcError {
    UpdateTimeout,
    InvalidDate,
}

#[derive(Clone, Copy)]
struct RawDateTime {
    second: u8,
    minute: u8,
    hour: u8,
    day: u8,
    month: u8,
    year: u8,
}

/// Read a stable date/time snapshot from the CMOS RTC.
pub fn read() -> Result<DateTime, RtcError> {
    let _guard = CMOS_LOCK.lock();
    for _ in 0..8 {
        wait_for_update_end()?;
        let raw = RawDateTime {
            second: read_register(REG_SECONDS),
            minute: read_register(REG_MINUTES),
            hour: read_register(REG_HOURS),
            day: read_register(REG_DAY),
            month: read_register(REG_MONTH),
            year: read_register(REG_YEAR),
        };
        let status_b = read_register(REG_STATUS_B);
        wait_for_update_end()?;
        if read_register(REG_SECONDS) == raw.second {
            return decode(raw, status_b);
        }
    }
    Err(RtcError::UpdateTimeout)
}

pub fn unix_seconds() -> Result<i64, RtcError> {
    read().map(unix_seconds_from_datetime)
}

fn unix_seconds_from_datetime(time: DateTime) -> i64 {
    let mut days = 0i64;
    for year in 1970..time.year {
        days += if year % 400 == 0 || year % 4 == 0 && year % 100 != 0 {
            366
        } else {
            365
        };
    }
    for month in 1..time.month {
        days += i64::from(days_in_month(time.year, month));
    }
    days += i64::from(time.day) - 1;
    days * 86_400
        + i64::from(time.hour) * 3_600
        + i64::from(time.minute) * 60
        + i64::from(time.second)
}

fn wait_for_update_end() -> Result<(), RtcError> {
    for _ in 0..MAX_READ_ATTEMPTS {
        if read_register(REG_STATUS_A) & UPDATE_IN_PROGRESS == 0 {
            return Ok(());
        }
        core::hint::spin_loop();
    }
    Err(RtcError::UpdateTimeout)
}

fn read_register(register: u8) -> u8 {
    unsafe {
        crate::arch::x86_64::io::outb(CMOS_INDEX, register & 0x7F);
        crate::arch::x86_64::io::inb(CMOS_DATA)
    }
}

fn decode(raw: RawDateTime, status_b: u8) -> Result<DateTime, RtcError> {
    let binary = status_b & BINARY_MODE != 0;
    let hour_24 = status_b & TWENTY_FOUR_HOUR_MODE != 0;
    let pm = raw.hour & PM_FLAG != 0;
    if hour_24 && pm {
        return Err(RtcError::InvalidDate);
    }
    let mut hour = convert(raw.hour & !PM_FLAG, binary)?;
    if !hour_24 {
        if !(1..=12).contains(&hour) {
            return Err(RtcError::InvalidDate);
        }
        hour %= 12;
        if pm {
            hour += 12;
        }
    }

    let year = convert(raw.year, binary)?;
    let year = if year <= 69 {
        2000 + year as u16
    } else {
        1900 + year as u16
    };
    let month = convert(raw.month, binary)?;
    let day = convert(raw.day, binary)?;
    let minute = convert(raw.minute, binary)?;
    let second = convert(raw.second, binary)?;

    if !(1..=12).contains(&month)
        || day == 0
        || day > days_in_month(year, month)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return Err(RtcError::InvalidDate);
    }

    Ok(DateTime {
        year,
        month,
        day,
        hour,
        minute,
        second,
    })
}

fn convert(value: u8, binary: bool) -> Result<u8, RtcError> {
    if binary {
        Ok(value)
    } else if value & 0x0F > 9 || value >> 4 > 9 {
        Err(RtcError::InvalidDate)
    } else {
        Ok((value & 0x0F) + ((value >> 4) * 10))
    }
}

fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        2 if year % 400 == 0 || year % 4 == 0 && year % 100 != 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

#[cfg(test)]
mod tests {
    use super::{DateTime, RawDateTime, RtcError, decode, unix_seconds_from_datetime};

    #[test]
    fn decodes_bcd_rtc_time_and_leap_day() {
        let time = decode(
            RawDateTime {
                second: 0x58,
                minute: 0x59,
                hour: 0x12,
                day: 0x29,
                month: 0x02,
                year: 0x24,
            },
            0x02,
        )
        .unwrap();
        assert_eq!(
            time,
            DateTime {
                year: 2024,
                month: 2,
                day: 29,
                hour: 12,
                minute: 59,
                second: 58,
            }
        );
    }

    #[test]
    fn decodes_binary_24_hour_time() {
        let time = decode(
            RawDateTime {
                second: 59,
                minute: 58,
                hour: 23,
                day: 31,
                month: 12,
                year: 25,
            },
            0x06,
        )
        .unwrap();
        assert_eq!(
            (time.year, time.month, time.day, time.hour),
            (2025, 12, 31, 23)
        );
    }

    #[test]
    fn converts_twelve_hour_midnight_and_noon() {
        for (raw_hour, expected) in [(0x12, 0), (0x92, 12), (0x81, 13)] {
            let time = decode(
                RawDateTime {
                    second: 0,
                    minute: 0,
                    hour: raw_hour,
                    day: 1,
                    month: 1,
                    year: 0x24,
                },
                0,
            )
            .unwrap();
            assert_eq!(time.hour, expected);
        }
    }

    #[test]
    fn rejects_invalid_calendar_dates() {
        assert_eq!(
            decode(
                RawDateTime {
                    second: 0,
                    minute: 0,
                    hour: 0,
                    day: 0x29,
                    month: 0x02,
                    year: 0x23,
                },
                0,
            ),
            Err(RtcError::InvalidDate)
        );
    }

    #[test]
    fn rejects_invalid_bcd_digits() {
        assert_eq!(
            decode(
                RawDateTime {
                    second: 0x1A,
                    minute: 0,
                    hour: 0,
                    day: 1,
                    month: 1,
                    year: 0x24,
                },
                0,
            ),
            Err(RtcError::InvalidDate)
        );
    }

    #[test]
    fn converts_rtc_dates_to_unix_seconds_across_leap_days() {
        assert_eq!(
            unix_seconds_from_datetime(DateTime {
                year: 1970,
                month: 1,
                day: 1,
                hour: 0,
                minute: 0,
                second: 0,
            }),
            0
        );
        assert_eq!(
            unix_seconds_from_datetime(DateTime {
                year: 2000,
                month: 2,
                day: 29,
                hour: 0,
                minute: 0,
                second: 0,
            }),
            951_782_400
        );
    }
}
