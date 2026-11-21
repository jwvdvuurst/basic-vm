//! Pure helpers behind the built-in function library (no VM or heap access).

/// SplitMix64: a small, fixed, seedable generator. The algorithm is part of
/// the documented behaviour, so a given seed always yields the same sequence.
pub(crate) struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    /// State used until the program calls `RANDOMIZE`.
    pub(crate) const DEFAULT_SEED: u64 = 0x853c_49e6_748f_ea9b;

    pub(crate) fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    pub(crate) fn reseed(&mut self, seed: u64) {
        self.state = seed;
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Uniform value with `0 <= x < 1` built from the top 53 bits.
    pub(crate) fn next_unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// Byte offset of the character at zero-based index `index`, or the string
/// length when `index` equals the character count. `None` when beyond that.
fn byte_offset(text: &str, index: usize) -> Option<usize> {
    text.char_indices()
        .map(|(offset, _)| offset)
        .chain(std::iter::once(text.len()))
        .nth(index)
}

pub(crate) fn left(text: &str, count: usize) -> &str {
    &text[..byte_offset(text, count).unwrap_or(text.len())]
}

pub(crate) fn right(text: &str, count: usize) -> &str {
    let total = text.chars().count();
    let skip = total.saturating_sub(count);
    &text[byte_offset(text, skip).unwrap_or(text.len())..]
}

/// `start` is one-based and at least 1; `length` of `None` means "to the end".
pub(crate) fn mid(text: &str, start: usize, length: Option<usize>) -> &str {
    let Some(from) = byte_offset(text, start - 1) else {
        return "";
    };
    let rest = &text[from..];
    match length {
        Some(length) => &rest[..byte_offset(rest, length).unwrap_or(rest.len())],
        None => rest,
    }
}

/// One-based character position of `needle` at or after one-based `start`,
/// or 0 when it does not occur.
pub(crate) fn instr(text: &str, needle: &str, start: usize) -> usize {
    let Some(from) = byte_offset(text, start - 1) else {
        return 0;
    };
    text[from..]
        .find(needle)
        .map_or(0, |found| text[..from + found].chars().count() + 1)
}

/// Converts days since 1970-01-01 to a proleptic Gregorian (year, month, day).
pub(crate) fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * shifted_month + 2) / 5 + 1) as u32;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    } as u32;
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// Formats Unix seconds as UTC `(date, time)` = (`YYYY-MM-DD`, `HH:MM:SS`).
pub(crate) fn format_utc(seconds: i64) -> (String, String) {
    let (year, month, day) = civil_from_days(seconds.div_euclid(86_400));
    let second_of_day = seconds.rem_euclid(86_400);
    (
        format!("{year:04}-{month:02}-{day:02}"),
        format!(
            "{:02}:{:02}:{:02}",
            second_of_day / 3600,
            second_of_day % 3600 / 60,
            second_of_day % 60
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn character_slicing_uses_unicode_scalars() {
        let text = "a\u{e9}\u{1f600}e\u{301}z";
        assert_eq!(text.chars().count(), 6);
        assert_eq!(left(text, 3), "a\u{e9}\u{1f600}");
        assert_eq!(left(text, 99), text);
        assert_eq!(right(text, 2), "\u{301}z");
        assert_eq!(right(text, 0), "");
        assert_eq!(mid(text, 2, Some(2)), "\u{e9}\u{1f600}");
        assert_eq!(mid(text, 3, None), "\u{1f600}e\u{301}z");
        assert_eq!(mid(text, 7, None), "");
        assert_eq!(mid(text, 8, Some(1)), "");
        assert_eq!(instr(text, "\u{1f600}", 1), 3);
        assert_eq!(instr(text, "z", 1), 6);
        assert_eq!(instr(text, "a", 2), 0);
        assert_eq!(instr(text, "", 7), 7);
        assert_eq!(instr(text, "", 8), 0);
    }

    #[test]
    fn civil_dates_match_known_instants() {
        assert_eq!(format_utc(0), ("1970-01-01".into(), "00:00:00".into()));
        assert_eq!(
            format_utc(951_782_400 + 3661),
            ("2000-02-29".into(), "01:01:01".into())
        );
        assert_eq!(
            format_utc(1_709_251_199),
            ("2024-02-29".into(), "23:59:59".into())
        );
        assert_eq!(format_utc(-1), ("1969-12-31".into(), "23:59:59".into()));
    }

    #[test]
    fn generator_is_seedable_and_in_unit_range() {
        let mut first = SplitMix64::new(7);
        let mut second = SplitMix64::new(7);
        let mut other = SplitMix64::new(8);
        let a: Vec<f64> = (0..50).map(|_| first.next_unit()).collect();
        let b: Vec<f64> = (0..50).map(|_| second.next_unit()).collect();
        let c: Vec<f64> = (0..50).map(|_| other.next_unit()).collect();
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert!(a.iter().all(|x| (0.0..1.0).contains(x)));
    }
}
