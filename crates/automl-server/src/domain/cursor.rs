//! Shared keyset-pagination cursor: `<rfc3339>|<uuid>` over `(created_at, id)`.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};

pub fn encode(created_at: DateTime<Utc>, id: Uuid) -> String {
    format!("{}|{id}", created_at.to_rfc3339())
}

pub fn decode(cursor: &str) -> ApiResult<(DateTime<Utc>, Uuid)> {
    let (ts, id) = cursor
        .split_once('|')
        .ok_or_else(|| ApiError::invalid("invalid cursor"))?;
    let created_at = DateTime::parse_from_rfc3339(ts)
        .map_err(|_| ApiError::invalid("invalid cursor timestamp"))?
        .with_timezone(&Utc);
    let id = Uuid::parse_str(id).map_err(|_| ApiError::invalid("invalid cursor id"))?;
    Ok((created_at, id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let ts = Utc::now();
        let id = Uuid::new_v4();
        let (ts2, id2) = decode(&encode(ts, id)).unwrap();
        assert_eq!(id, id2);
        assert_eq!(ts.timestamp_micros(), ts2.timestamp_micros());
    }

    #[test]
    fn rejects_malformed() {
        assert!(decode("nope").is_err());
        assert!(decode("2020-01-01T00:00:00Z|not-a-uuid").is_err());
        assert!(decode("yesterday|00000000-0000-0000-0000-000000000000").is_err());
    }
}
