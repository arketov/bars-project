//! Время в RFC 3339 (UTC) без внешних зависимостей: для `projects.toml` и ответов `status`.

use std::time::{SystemTime, UNIX_EPOCH};

/// `2026-10-15T12:30:00Z`; время до 1970 года показывается как начало эпохи.
pub fn rfc3339(time: SystemTime) -> String {
    let seconds = time
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    let days = i64::try_from(seconds / 86_400).unwrap_or(0);
    let rest = seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// Токен успешного обновления индекса для «Новых тем»: секунды и наносекунды Unix одним числом.
/// В отличие от `rfc3339` различает два обновления внутри одной секунды (design.md, «Жизненный цикл
/// темы и проверки»); токен только сравнивается на равенство и пользователю не показывается.
pub fn index_token(time: SystemTime) -> String {
    let elapsed = time.duration_since(UNIX_EPOCH).unwrap_or_default();
    format!("{}.{:09}", elapsed.as_secs(), elapsed.subsec_nanos())
}

/// Григорианская дата по числу дней от 1970-01-01 (алгоритм Говарда Хиннанта).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_part = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_part + 2) / 5 + 1;
    let month = if month_part < 10 {
        month_part + 3
    } else {
        month_part - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn epoch_start() {
        assert_eq!(rfc3339(UNIX_EPOCH), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn known_moments() {
        // 2026-10-15T12:30:45Z
        let time = UNIX_EPOCH + Duration::from_secs(1_792_067_445);
        assert_eq!(rfc3339(time), "2026-10-15T12:30:45Z");
        // 29 февраля високосного года
        let leap = UNIX_EPOCH + Duration::from_secs(1_709_164_800);
        assert_eq!(rfc3339(leap), "2024-02-29T00:00:00Z");
    }

    #[test]
    fn index_token_tells_apart_updates_within_one_second() {
        let first = UNIX_EPOCH + Duration::new(1_792_067_445, 100);
        let second = UNIX_EPOCH + Duration::new(1_792_067_445, 200_000_000);
        // Секундная метка их не различает, токен — различает.
        assert_eq!(rfc3339(first), rfc3339(second));
        assert_ne!(index_token(first), index_token(second));
        assert_eq!(index_token(first), index_token(first));
        assert_eq!(index_token(first), "1792067445.000000100");
    }
}
