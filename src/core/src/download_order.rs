use std::sync::atomic::{AtomicU64, Ordering};

static DOWNLOAD_COUNT: AtomicU64 = AtomicU64::new(0);

pub fn get_next_download_order() -> u64 {
    DOWNLOAD_COUNT.fetch_add(1, Ordering::Relaxed) + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counter_increments() {
        let a = get_next_download_order();
        let b = get_next_download_order();
        assert!(b > a);
    }
}
