//! Wire helpers: byte strings travel as unpadded base64url inside JSON.

use std::fmt;

use data_encoding::BASE64URL_NOPAD;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Bytes encoded as unpadded base64url in JSON.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct B64(pub Vec<u8>);

impl B64 {
    /// Wrap bytes.
    pub fn of(b: impl Into<Vec<u8>>) -> Self {
        Self(b.into())
    }
}

impl fmt::Debug for B64 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "B64({} bytes)", self.0.len())
    }
}

impl Serialize for B64 {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&BASE64URL_NOPAD.encode(&self.0))
    }
}

impl<'de> Deserialize<'de> for B64 {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        BASE64URL_NOPAD
            .decode(s.as_bytes())
            .map(B64)
            .map_err(|_| serde::de::Error::custom("invalid base64url"))
    }
}

/// Encode bytes as unpadded base64url.
pub fn b64(b: &[u8]) -> String {
    BASE64URL_NOPAD.encode(b)
}

/// Decode unpadded base64url.
pub fn unb64(s: &str) -> Option<Vec<u8>> {
    BASE64URL_NOPAD.decode(s.as_bytes()).ok()
}
