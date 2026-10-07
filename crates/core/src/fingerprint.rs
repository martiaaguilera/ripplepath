use std::fmt;

use serde::{Deserialize, Serialize};

/// Content fingerprint of a symbol's own tokens (comments and nested members excluded).
///
/// 128 bits of BLAKE3 is far beyond collision concerns for per-repository symbol counts and keeps
/// JSON output compact.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub struct Fingerprint([u8; 16]);

impl fmt::Debug for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Fingerprint({self})")
    }
}

impl fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl From<Fingerprint> for String {
    fn from(value: Fingerprint) -> Self {
        value.to_string()
    }
}

impl TryFrom<String> for Fingerprint {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.len() != 32 {
            return Err(format!("fingerprint must be 32 hex chars, got {}", value.len()));
        }
        let mut bytes = [0u8; 16];
        for (i, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&value[i * 2..i * 2 + 2], 16)
                .map_err(|e| format!("invalid fingerprint hex: {e}"))?;
        }
        Ok(Self(bytes))
    }
}

/// Feeds tokens into the hash with a separator so that `["ab", "c"]` and `["a", "bc"]` differ.
pub struct FingerprintBuilder(blake3::Hasher);

impl FingerprintBuilder {
    pub fn new() -> Self {
        Self(blake3::Hasher::new())
    }

    pub fn token(&mut self, token: &str) {
        self.0.update(&(token.len() as u64).to_le_bytes());
        self.0.update(token.as_bytes());
    }

    pub fn finish(self) -> Fingerprint {
        let hash = self.0.finalize();
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(&hash.as_bytes()[..16]);
        Fingerprint(bytes)
    }
}

impl Default for FingerprintBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_boundaries_matter() {
        let mut a = FingerprintBuilder::new();
        a.token("ab");
        a.token("c");
        let mut b = FingerprintBuilder::new();
        b.token("a");
        b.token("bc");
        assert_ne!(a.finish(), b.finish());
    }

    #[test]
    fn hex_round_trip() {
        let mut builder = FingerprintBuilder::new();
        builder.token("x");
        let fp = builder.finish();
        let parsed = Fingerprint::try_from(fp.to_string());
        assert_eq!(parsed, Ok(fp));
    }
}
