use anyhow::{bail, Result};

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::*;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;

pub fn get(key: &str) -> Result<Option<String>> {
    Ok(list()?.get(key).cloned())
}

pub fn validate_key(key: &str) -> Result<()> {
    let mut chars = key.chars();
    let valid = matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !valid {
        bail!("invalid name '{key}': use letters, digits and '_', and do not start with a digit");
    }
    Ok(())
}

pub fn validate_value(value: &str) -> Result<()> {
    if value.contains(['\n', '\r', '\0']) {
        bail!("invalid value: newlines and NUL characters are not allowed");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_keys() {
        for key in ["FOO", "_foo", "a1_B2"] {
            assert!(validate_key(key).is_ok(), "{key}");
        }
    }

    #[test]
    fn rejects_invalid_keys() {
        for key in ["", "1FOO", "FOO-BAR", "FOO BAR", "A=B", "é"] {
            assert!(validate_key(key).is_err(), "{key}");
        }
    }

    #[test]
    fn accepts_plain_values() {
        assert!(validate_value("").is_ok());
        assert!(validate_value("a b=c \"q\" $x").is_ok());
    }

    #[test]
    fn rejects_control_characters_in_values() {
        for value in ["a\nb", "a\rb", "a\0b"] {
            assert!(validate_value(value).is_err());
        }
    }
}
