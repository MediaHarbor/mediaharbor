use std::time::{SystemTime, UNIX_EPOCH};

use uuid::Uuid;

pub fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn now_secs_f64() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

pub fn rand_hex(n: usize) -> String {
    let mut out = String::with_capacity(n + 32);
    while out.len() < n {
        out.push_str(&Uuid::new_v4().simple().to_string());
    }
    out.truncate(n);
    out
}

pub fn rand_uuid() -> String {
    Uuid::new_v4().to_string()
}

pub fn cpn() -> String {
    const ALPHA: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_";
    let mut out = String::with_capacity(16);
    while out.len() < 16 {
        for (i, b) in Uuid::new_v4().as_bytes().iter().enumerate() {
            if i == 6 || i == 8 || out.len() >= 16 {
                continue;
            }
            out.push(ALPHA[*b as usize % ALPHA.len()] as char);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_secs_tracks_now_millis() {
        let s = now_secs();
        let m = now_millis();
        assert!((m / 1000).abs_diff(s as u128) <= 1);
    }

    #[test]
    fn rand_hex_has_requested_length() {
        for n in [1, 16, 32, 40, 100] {
            let h = rand_hex(n);
            assert_eq!(h.len(), n);
            assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
        }
    }

    #[test]
    fn rand_uuid_is_v4_and_unique() {
        let a = rand_uuid();
        assert_eq!(a.len(), 36);
        assert_eq!(a.as_bytes()[14], b'4');
        assert_ne!(a, rand_uuid());
    }

    #[test]
    fn cpn_is_16_url_safe_chars() {
        const ALPHA: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_";
        let c = cpn();
        assert_eq!(c.chars().count(), 16);
        assert!(c.chars().all(|ch| ALPHA.contains(ch)));
    }
}
