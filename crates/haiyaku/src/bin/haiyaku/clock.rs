//! The time a run is recorded at.

use std::time::{SystemTime, UNIX_EPOCH};

/// Now, as [`rfc3339_utc`] writes it.
pub fn now() -> String {
    // A clock set before 1970 is broken, and the time is a record that nothing is looked up by: the epoch
    // stands in for it.
    let seconds = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs());
    rfc3339_utc(seconds)
}

/// `seconds_since_epoch` as an RFC 3339 time in UTC, to the second, as in `2024-02-29T23:59:59Z`.
pub fn rfc3339_utc(seconds_since_epoch: u64) -> String {
    let (days, second) = (seconds_since_epoch / 86_400, seconds_since_epoch % 86_400);
    let (year, month, day) = civil(days);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z", second / 3600, second / 60 % 60, second % 60)
}

/// The Gregorian year, month and day `days` days after 1970-01-01, by Howard Hinnant's `civil_from_days`.
fn civil(days: u64) -> (u64, u64, u64) {
    // Counted from 0000-03-01, so that a leap day is the last day of its year and every 400 years, an era,
    // have the same 146 097 days.
    let from_march = days + 719_468;
    let era = from_march / 146_097;
    let day_of_era = from_march % 146_097;
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    // The months from March run 31, 30, 31, 30, 31 days and again, which 153 days to each 5 months gives.
    let month_from_march = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_from_march + 2) / 5 + 1;
    let month = if month_from_march < 10 { month_from_march + 3 } else { month_from_march - 9 };
    // January and February end the year counted from March, and so belong to the next calendar year.
    (era * 400 + year_of_era + u64::from(month <= 2), month, day)
}

#[cfg(test)]
mod tests {
    use super::rfc3339_utc;

    #[test]
    fn the_epoch() {
        assert_eq!(rfc3339_utc(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn leap_days() {
        // 2000 leaps although a century: it divides by 400.
        assert_eq!(rfc3339_utc(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(rfc3339_utc(1_709_251_199), "2024-02-29T23:59:59Z");
        assert_eq!(rfc3339_utc(1_709_251_200), "2024-03-01T00:00:00Z");
    }

    #[test]
    fn ends_of_years_and_a_century_that_does_not_leap() {
        assert_eq!(rfc3339_utc(946_684_799), "1999-12-31T23:59:59Z");
        assert_eq!(rfc3339_utc(1_704_067_199), "2023-12-31T23:59:59Z");
        assert_eq!(rfc3339_utc(4_107_542_399), "2100-02-28T23:59:59Z");
        assert_eq!(rfc3339_utc(4_107_542_400), "2100-03-01T00:00:00Z");
    }
}
