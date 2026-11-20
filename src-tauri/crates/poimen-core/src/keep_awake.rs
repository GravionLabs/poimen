//! `agents.keepAwake` (#987): hold a sleep inhibitor while an agent works or a run runs.

use serde_json::Value;

/// Absent and `null` are off; anything but a boolean is an error with the reason (the setting
/// then counts as off).
pub fn keep_awake(root: &Value) -> Result<bool, String> {
    match root.get("agents").and_then(|a| a.get("keepAwake")) {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err("agents.keepAwake must be true or false".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keep_awake_is_a_boolean_and_off_by_default() {
        let v = |t: &str| serde_json::from_str::<Value>(t).unwrap();
        assert_eq!(keep_awake(&v("{}")), Ok(false));
        assert_eq!(
            keep_awake(&v(r#"{"agents":{"keepAwake":null}}"#)),
            Ok(false)
        );
        assert_eq!(keep_awake(&v(r#"{"agents":{"keepAwake":true}}"#)), Ok(true));
        assert_eq!(
            keep_awake(&v(r#"{"agents":{"keepAwake":false}}"#)),
            Ok(false)
        );
        assert!(keep_awake(&v(r#"{"agents":{"keepAwake":"yes"}}"#)).is_err());
    }
}
