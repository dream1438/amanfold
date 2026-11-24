//! TOTP one-time codes (RFC 6238), from a base32 secret or `otpauth://` URI.

use hmac::{Hmac, Mac};
use zeroize::Zeroizing;

use crate::{Error, Result};

/// Hash algorithm for TOTP.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Algorithm {
    /// HMAC-SHA1 (the default almost everywhere).
    Sha1,
    /// HMAC-SHA256.
    Sha256,
}

/// TOTP configuration.
#[derive(Clone, PartialEq, Eq)]
pub struct Totp {
    secret: Zeroizing<Vec<u8>>,
    /// Number of digits (6-8).
    pub digits: u32,
    /// Time step in seconds.
    pub period: u64,
    /// Hash algorithm.
    pub algorithm: Algorithm,
}

impl core::fmt::Debug for Totp {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "Totp({} digits, {}s, {:?})",
            self.digits, self.period, self.algorithm
        )
    }
}

fn decode_base32(s: &str) -> Result<Zeroizing<Vec<u8>>> {
    let cleaned: String = s
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-' && *c != '=')
        .map(|c| c.to_ascii_uppercase())
        .collect();
    let v = data_encoding::BASE32_NOPAD
        .decode(cleaned.as_bytes())
        .map_err(|_| Error::InvalidCode("totp secret"))?;
    if v.is_empty() {
        return Err(Error::InvalidCode("totp secret"));
    }
    Ok(Zeroizing::new(v))
}

impl Totp {
    /// Parse a bare base32 secret or an `otpauth://totp/...` URI.
    pub fn parse(input: &str) -> Result<Self> {
        let input = input.trim();
        let mut t = Self {
            secret: Zeroizing::new(Vec::new()),
            digits: 6,
            period: 30,
            algorithm: Algorithm::Sha1,
        };
        if let Some(rest) = input.strip_prefix("otpauth://totp/") {
            let query = rest
                .split_once('?')
                .map(|(_, q)| q)
                .ok_or(Error::InvalidCode("totp uri"))?;
            let mut secret = None;
            for pair in query.split('&') {
                let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
                match k.to_ascii_lowercase().as_str() {
                    "secret" => secret = Some(decode_base32(v)?),
                    "digits" => {
                        t.digits = v.parse().map_err(|_| Error::InvalidCode("totp digits"))?
                    }
                    "period" => {
                        t.period = v.parse().map_err(|_| Error::InvalidCode("totp period"))?
                    }
                    "algorithm" => {
                        t.algorithm = match v.to_ascii_uppercase().as_str() {
                            "SHA1" => Algorithm::Sha1,
                            "SHA256" => Algorithm::Sha256,
                            _ => return Err(Error::InvalidCode("totp algorithm")),
                        }
                    }
                    _ => {}
                }
            }
            t.secret = secret.ok_or(Error::InvalidCode("totp secret"))?;
        } else {
            t.secret = decode_base32(input)?;
        }
        if !(6..=8).contains(&t.digits) || t.period == 0 || t.period > 300 {
            return Err(Error::InvalidCode("totp parameters"));
        }
        Ok(t)
    }

    /// Code for a Unix time in seconds.
    pub fn code_at(&self, unix_secs: u64) -> String {
        let counter = (unix_secs / self.period).to_be_bytes();
        let mac = match self.algorithm {
            Algorithm::Sha1 => {
                let mut m =
                    Hmac::<sha1::Sha1>::new_from_slice(&self.secret).expect("any key length");
                m.update(&counter);
                m.finalize().into_bytes().to_vec()
            }
            Algorithm::Sha256 => {
                let mut m =
                    Hmac::<sha2::Sha256>::new_from_slice(&self.secret).expect("any key length");
                m.update(&counter);
                m.finalize().into_bytes().to_vec()
            }
        };
        let off = usize::from(mac[mac.len() - 1] & 0x0f);
        let bin = u32::from_be_bytes([mac[off] & 0x7f, mac[off + 1], mac[off + 2], mac[off + 3]]);
        format!(
            "{:0width$}",
            bin % 10u32.pow(self.digits),
            width = self.digits as usize
        )
    }

    /// Seconds until the current code changes.
    pub fn seconds_remaining(&self, unix_secs: u64) -> u64 {
        self.period - unix_secs % self.period
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // RFC 6238 Appendix B test vectors (8 digits).
    #[test]
    fn rfc6238_sha1() {
        let secret = data_encoding::BASE32_NOPAD.encode(b"12345678901234567890");
        let t = Totp::parse(&format!("otpauth://totp/x?secret={secret}&digits=8")).unwrap();
        assert_eq!(t.code_at(59), "94287082");
        assert_eq!(t.code_at(1111111109), "07081804");
        assert_eq!(t.code_at(20000000000), "65353130");
    }

    #[test]
    fn rfc6238_sha256() {
        let secret = data_encoding::BASE32_NOPAD.encode(b"12345678901234567890123456789012");
        let t = Totp::parse(&format!(
            "otpauth://totp/x?secret={secret}&digits=8&algorithm=SHA256"
        ))
        .unwrap();
        assert_eq!(t.code_at(59), "46119246");
        assert_eq!(t.code_at(1111111109), "68084774");
    }

    #[test]
    fn bare_secret_and_errors() {
        let t = Totp::parse("JBSW Y3DP EHPK 3PXP").unwrap();
        assert_eq!(t.code_at(0).len(), 6);
        assert_eq!(t.seconds_remaining(29), 1);
        assert!(Totp::parse("not base32!").is_err());
        assert!(Totp::parse("otpauth://totp/x?digits=6").is_err());
        assert!(!format!("{t:?}").contains("JBSW"));
    }
}
