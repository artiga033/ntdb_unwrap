//! Process-global pending-byte control. Set only before opening any connection.
use rusqlite::ffi;
pub const STANDARD: u32 = 0x40000000;
pub fn value(offset: i64) -> Result<u32, String> {
    let value = i64::from(STANDARD)
        .checked_add(offset)
        .filter(|v| (1..=i64::from(u32::MAX)).contains(v))
        .ok_or_else(|| "pending-byte offset must resolve to 1..=u32::MAX".to_string())?;
    Ok(value as u32)
}
pub fn current() -> u32 {
    // Query only: passing zero does not modify SQLite's global configuration.
    unsafe { ffi::sqlite3_test_control(11, 0u32) as u32 }
}
/// # Safety
/// No database connection may have been opened in this process before this call.
pub unsafe fn configure(offset: Option<i64>) -> Result<(), String> {
    configure_control(
        offset,
        |target| unsafe {
            ffi::sqlite3_test_control(11, target);
        },
        current,
    )
}
fn configure_control(
    offset: Option<i64>,
    mut set: impl FnMut(u32),
    mut query: impl FnMut() -> u32,
) -> Result<(), String> {
    if let Some(offset) = offset {
        let target = value(offset)?;
        if offset != 0 {
            set(target);
        }
        if query() != target {
            return Err("SQLITE_TESTCTRL_PENDING_BYTE unavailable or ineffective".into());
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounds() {
        assert_eq!(value(0).unwrap(), STANDARD);
        assert_eq!(value(-1024).unwrap(), STANDARD - 1024);
        assert_eq!(value(1).unwrap(), STANDARD + 1);
        assert!(value(-i64::from(STANDARD)).is_err());
        assert!(value(i64::MAX).is_err());
        assert!(value(i64::MIN).is_err());
        assert_eq!(
            value(i64::from(u32::MAX) - i64::from(STANDARD)).unwrap(),
            u32::MAX
        );
    }
    #[test]
    fn default_does_not_touch_control_and_unsupported_fails() {
        configure_control(
            None,
            |_| panic!("default wrote global config"),
            || panic!("default queried global config"),
        )
        .unwrap();
        configure_control(Some(0), |_| panic!("zero wrote global config"), || STANDARD).unwrap();
        assert!(configure_control(Some(-1024), |_| {}, || STANDARD).is_err());
        assert!(configure_control(Some(-1024), |_| {}, || 0).is_err());
        let value = std::cell::Cell::new(STANDARD);
        configure_control(Some(-1024), |target| value.set(target), || value.get()).unwrap();
    }
}
