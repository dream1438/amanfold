//! Password and passphrase generator using the OS CSPRNG with unbiased sampling.

use rand_core::{OsRng, RngCore};

use crate::{Error, Result, secret::SecretString};

/// Character classes for random passwords.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PasswordRules {
    /// Total length.
    pub length: usize,
    /// Include a-z.
    pub lowercase: bool,
    /// Include A-Z.
    pub uppercase: bool,
    /// Include 0-9.
    pub digits: bool,
    /// Include symbols.
    pub symbols: bool,
    /// Leave out look-alike characters (0 O o 1 l I).
    pub avoid_ambiguous: bool,
}

impl Default for PasswordRules {
    fn default() -> Self {
        Self {
            length: 20,
            lowercase: true,
            uppercase: true,
            digits: true,
            symbols: true,
            avoid_ambiguous: true,
        }
    }
}

/// Uniform random integer in `0..n` (rejection sampling, no modulo bias).
fn uniform(n: u32) -> u32 {
    assert!(n > 0);
    let zone = u32::MAX - (u32::MAX % n);
    loop {
        let x = OsRng.next_u32();
        if x < zone {
            return x % n;
        }
    }
}

/// Generate a random password. Every enabled class appears at least once.
pub fn password(rules: &PasswordRules) -> Result<SecretString> {
    const AMBIG: &str = "0Oo1lI";
    let classes: Vec<Vec<char>> = [
        (rules.lowercase, "abcdefghijklmnopqrstuvwxyz"),
        (rules.uppercase, "ABCDEFGHIJKLMNOPQRSTUVWXYZ"),
        (rules.digits, "0123456789"),
        (rules.symbols, "!@#$%^&*()-_=+[]{};:,.?/"),
    ]
    .into_iter()
    .filter(|(on, _)| *on)
    .map(|(_, s)| {
        s.chars()
            .filter(|c| !(rules.avoid_ambiguous && AMBIG.contains(*c)))
            .collect()
    })
    .collect();
    if classes.is_empty() {
        return Err(Error::InvalidArgument("no character classes"));
    }
    if rules.length < classes.len().max(8) || rules.length > 256 {
        return Err(Error::InvalidArgument("password length"));
    }
    let all: Vec<char> = classes.iter().flatten().copied().collect();
    let mut out: Vec<char> = classes
        .iter()
        .map(|c| c[uniform(c.len() as u32) as usize])
        .collect();
    while out.len() < rules.length {
        out.push(all[uniform(all.len() as u32) as usize]);
    }
    // Fisher-Yates shuffle so the guaranteed characters are not at the front.
    for i in (1..out.len()).rev() {
        out.swap(i, uniform(i as u32 + 1) as usize);
    }
    Ok(SecretString::new(out.into_iter().collect::<String>()))
}

/// Generate a passphrase of `words` words from the 2048-word BIP-39 list
/// (11 bits each), joined by `separator`.
pub fn passphrase(words: usize, separator: &str) -> Result<SecretString> {
    if !(4..=20).contains(&words) {
        return Err(Error::InvalidArgument("word count"));
    }
    let list = bip39::Language::English.word_list();
    let picked: Vec<&str> = (0..words)
        .map(|_| list[uniform(list.len() as u32) as usize])
        .collect();
    Ok(SecretString::new(picked.join(separator)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_meets_rules() {
        for _ in 0..200 {
            let p = password(&PasswordRules::default()).unwrap();
            let s = p.expose();
            assert_eq!(s.chars().count(), 20);
            assert!(s.chars().any(|c| c.is_ascii_lowercase()));
            assert!(s.chars().any(|c| c.is_ascii_uppercase()));
            assert!(s.chars().any(|c| c.is_ascii_digit()));
            assert!(s.chars().any(|c| !c.is_ascii_alphanumeric()));
            assert!(!s.chars().any(|c| "0Oo1lI".contains(c)));
        }
    }

    #[test]
    fn digits_only_pin() {
        let r = PasswordRules {
            length: 8,
            lowercase: false,
            uppercase: false,
            symbols: false,
            ..Default::default()
        };
        assert!(
            password(&r)
                .unwrap()
                .expose()
                .chars()
                .all(|c| c.is_ascii_digit())
        );
    }

    #[test]
    fn bad_rules() {
        let none = PasswordRules {
            lowercase: false,
            uppercase: false,
            digits: false,
            symbols: false,
            ..Default::default()
        };
        assert!(password(&none).is_err());
        assert!(
            password(&PasswordRules {
                length: 4,
                ..Default::default()
            })
            .is_err()
        );
    }

    #[test]
    fn uniform_is_roughly_uniform() {
        let mut counts = [0u32; 6];
        for _ in 0..60_000 {
            counts[uniform(6) as usize] += 1;
        }
        assert!(
            counts.iter().all(|&c| (9_000..11_000).contains(&c)),
            "{counts:?}"
        );
    }

    #[test]
    fn passphrase_shape() {
        let p = passphrase(6, "-").unwrap();
        assert_eq!(p.expose().split('-').count(), 6);
        assert!(passphrase(2, "-").is_err());
    }
}
