//! Payment card helpers: number validation, brand detection, expiry parsing.
//!
//! Accepts Arabic-Indic (٠-٩) and Persian (۰-۹) digits as well as ASCII, so
//! cards can be typed on any keyboard.

use crate::{Error, Result};

/// Card network, detected from the number prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Brand {
    /// Visa
    Visa,
    /// Mastercard
    Mastercard,
    /// American Express
    Amex,
    /// Discover
    Discover,
    /// Diners Club
    Diners,
    /// JCB
    Jcb,
    /// UnionPay
    UnionPay,
    /// Maestro
    Maestro,
    /// Not recognised (still allowed if the Luhn check passes).
    Unknown,
}

fn ascii_digit(c: char) -> Option<char> {
    match c {
        '0'..='9' => Some(c),
        '\u{660}'..='\u{669}' => char::from_digit(c as u32 - 0x660, 10),
        '\u{6f0}'..='\u{6f9}' => char::from_digit(c as u32 - 0x6f0, 10),
        _ => None,
    }
}

/// Convert any supported digits to ASCII, dropping spaces and dashes.
/// Returns an error on any other character.
pub fn to_ascii_digits(s: &str) -> Result<String> {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_whitespace() || c == '-' {
            continue;
        }
        out.push(ascii_digit(c).ok_or(Error::InvalidArgument("not a number"))?);
    }
    Ok(out)
}

/// Luhn checksum.
pub fn luhn_valid(digits: &str) -> bool {
    let mut sum = 0u32;
    for (i, c) in digits.bytes().rev().enumerate() {
        if !c.is_ascii_digit() {
            return false;
        }
        let mut d = u32::from(c - b'0');
        if i % 2 == 1 {
            d *= 2;
            if d > 9 {
                d -= 9;
            }
        }
        sum += d;
    }
    !digits.is_empty() && sum % 10 == 0
}

/// Validate and normalise a card number to ASCII digits.
pub fn normalise_number(s: &str) -> Result<String> {
    let d = to_ascii_digits(s)?;
    if !(12..=19).contains(&d.len()) {
        return Err(Error::InvalidArgument("card number length"));
    }
    if !luhn_valid(&d) {
        return Err(Error::InvalidArgument("card number checksum"));
    }
    Ok(d)
}

/// Detect the card brand from the leading digits.
pub fn brand(number: &str) -> Brand {
    let p = |n: usize| {
        number
            .get(..n)
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0)
    };
    match () {
        _ if p(2) == 34 || p(2) == 37 => Brand::Amex,
        _ if (51..=55).contains(&p(2)) || (2221..=2720).contains(&p(4)) => Brand::Mastercard,
        _ if p(4) == 6011 || p(2) == 65 || (644..=649).contains(&p(3)) => Brand::Discover,
        _ if (3528..=3589).contains(&p(4)) => Brand::Jcb,
        _ if p(2) == 36 || p(2) == 38 || (300..=305).contains(&p(3)) => Brand::Diners,
        _ if p(2) == 62 => Brand::UnionPay,
        _ if p(1) == 4 => Brand::Visa,
        _ if [50, 56, 57, 58].contains(&p(2)) || p(2) == 67 => Brand::Maestro,
        _ => Brand::Unknown,
    }
}

/// Masked display form, for example `•••• 1111`.
pub fn masked(number: &str) -> String {
    let n = number.len();
    format!("•••• {}", number.get(n.saturating_sub(4)..).unwrap_or(""))
}

/// Parse an expiry month (1-12).
pub fn parse_month(s: &str) -> Result<u8> {
    let d = to_ascii_digits(s)?;
    match d.parse::<u8>() {
        Ok(m @ 1..=12) => Ok(m),
        _ => Err(Error::InvalidArgument("expiry month")),
    }
}

/// Parse an expiry year. Two-digit years are read as 20xx.
pub fn parse_year(s: &str) -> Result<u16> {
    let d = to_ascii_digits(s)?;
    match (d.len(), d.parse::<u16>()) {
        (2, Ok(y)) => Ok(2000 + y),
        (4, Ok(y @ 2000..=2099)) => Ok(y),
        _ => Err(Error::InvalidArgument("expiry year")),
    }
}

/// Whether a card with this expiry is expired at (`year`, `month`).
/// Cards are valid through the last day of their expiry month.
pub fn is_expired(exp_year: u16, exp_month: u8, year: u16, month: u8) -> bool {
    (exp_year, exp_month) < (year, month)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn luhn() {
        assert!(luhn_valid("4111111111111111"));
        assert!(luhn_valid("378282246310005"));
        assert!(!luhn_valid("4111111111111112"));
        assert!(!luhn_valid(""));
    }

    #[test]
    fn arabic_digits() {
        assert_eq!(
            normalise_number("٤١١١ ١١١١ ١١١١ ١١١١").unwrap(),
            "4111111111111111"
        );
        assert_eq!(
            normalise_number("۴۱۱۱-۱۱۱۱-۱۱۱۱-۱۱۱۱").unwrap(),
            "4111111111111111"
        );
        assert_eq!(parse_month("٠٧").unwrap(), 7);
    }

    #[test]
    fn brands() {
        assert_eq!(brand("4111111111111111"), Brand::Visa);
        assert_eq!(brand("5555555555554444"), Brand::Mastercard);
        assert_eq!(brand("2223003122003222"), Brand::Mastercard);
        assert_eq!(brand("378282246310005"), Brand::Amex);
        assert_eq!(brand("6011111111111117"), Brand::Discover);
        assert_eq!(brand("3530111333300000"), Brand::Jcb);
        assert_eq!(brand("30569309025904"), Brand::Diners);
        assert_eq!(brand("6200000000000005"), Brand::UnionPay);
    }

    #[test]
    fn expiry() {
        assert_eq!(parse_year("29").unwrap(), 2029);
        assert!(parse_month("0").is_err() && parse_month("13").is_err());
        assert!(is_expired(2026, 10, 2026, 11));
        assert!(!is_expired(2026, 11, 2026, 11));
    }

    #[test]
    fn masking() {
        assert_eq!(masked("4111111111111111"), "•••• 1111");
    }

    #[test]
    fn rejects_garbage() {
        assert!(normalise_number("4111x111").is_err());
        assert!(normalise_number("4242").is_err());
    }
}
