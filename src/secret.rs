use std::fmt;

use zeroize::{Zeroize, ZeroizeOnDrop};

const REDACTED_SECRET: &str = "<redacted>";

/// A redacting, zero-on-drop wrapper for sensitive values.
///
/// The wrapped value is zeroized when dropped. [`Debug`](fmt::Debug) and
/// [`Display`](fmt::Display) never expose the inner value; use [`Self::expose`]
/// only at the boundary where the cleartext is required. Other cleartext copies
/// made before wrapping or after exposing are outside this wrapper's control.
#[derive(Clone, ZeroizeOnDrop)]
pub struct Secret<T: Zeroize>(T);

impl<T: Zeroize> Secret<T> {
    /// Wrap a sensitive value.
    pub fn new(value: T) -> Self {
        Self(value)
    }

    /// Borrow the inner secret value.
    pub fn expose(&self) -> &T {
        &self.0
    }
}

impl<T: Zeroize> fmt::Debug for Secret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED_SECRET)
    }
}

impl<T: Zeroize> fmt::Display for Secret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED_SECRET)
    }
}

impl From<String> for Secret<String> {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for Secret<String> {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl From<Vec<u8>> for Secret<Vec<u8>> {
    fn from(value: Vec<u8>) -> Self {
        Self(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_and_display_are_redacted() {
        let secret = Secret::from("super-secret-key");

        assert_eq!(format!("{secret:?}"), REDACTED_SECRET);
        assert_eq!(format!("{secret}"), REDACTED_SECRET);
    }
}
