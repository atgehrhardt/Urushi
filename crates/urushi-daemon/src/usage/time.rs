//! RFC 3339 timestamps (as usage endpoints send them) → milliseconds since the epoch.

/// Parses `2026-11-13T05:59:59.531229+00:00` (or `…Z`). `None` if malformed.
pub(crate) fn rfc3339_ms(s: &str) -> Option<i64> {
    let s = s.trim();
    let (date, rest) = s.split_once(['T', 't', ' '])?;
    let mut d = date.split('-');
    let year: i64 = d.next()?.parse().ok()?;
    let month: i64 = d.next()?.parse().ok()?;
    let day: i64 = d.next()?.parse().ok()?;
    if d.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    // Split the time from its offset: `Z`, `+hh:mm` or `-hh:mm`.
    let (time, offset_minutes) = if let Some(time) = rest.strip_suffix(['Z', 'z']) {
        (time, 0)
    } else {
        let at = rest.rfind(['+', '-'])?;
        let (time, offset) = rest.split_at(at);
        let sign = if offset.starts_with('-') { -1 } else { 1 };
        let (h, m) = offset[1..].split_once(':')?;
        let minutes = h.parse::<i64>().ok()? * 60 + m.parse::<i64>().ok()?;
        (time, sign * minutes)
    };
    let mut t = time.split(':');
    let hour: i64 = t.next()?.parse().ok()?;
    let minute: i64 = t.next()?.parse().ok()?;
    let seconds = t.next()?;
    let (whole, fraction) = seconds.split_once('.').unwrap_or((seconds, ""));
    let second: i64 = whole.parse().ok()?;
    let millis: i64 = format!("{fraction:0<3}")
        .get(..3)
        .and_then(|f| f.parse().ok())
        .unwrap_or(0);
    if hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let days = days_from_civil(year, month, day);
    let secs = days * 86_400 + hour * 3_600 + minute * 60 + second - offset_minutes * 60;
    Some(secs * 1_000 + millis)
}

/// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_offsets_fractions_and_zulu() {
        assert_eq!(rfc3339_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(rfc3339_ms("2000-03-01T00:00:00Z"), Some(951_868_800_000));
        assert_eq!(
            rfc3339_ms("2026-11-13T05:59:59.531229+00:00"),
            Some(1_794_549_599_531)
        );
        assert_eq!(
            rfc3339_ms("2026-11-13T07:59:59.5+02:00"),
            rfc3339_ms("2026-11-13T05:59:59.500Z")
        );
        assert_eq!(rfc3339_ms("garbage"), None);
        assert_eq!(rfc3339_ms("2026-13-01T00:00:00Z"), None);
    }
}
