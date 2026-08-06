use regex::Regex;
use sha2::{Digest, Sha256};

pub fn fingerprint(secret: &[u8]) -> String {
    let digest = Sha256::digest(secret);
    let short = digest[..4]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join("");
    format!("sha256:{short}")
}

pub fn redact(line: &str) -> String {
    let prefixed = Regex::new(r"\b(ghp|gho|github_pat)_[A-Za-z0-9_]{12,}\b").unwrap();
    let long_runs = Regex::new(r"\b[A-Fa-f0-9]{40,}\b|\b[A-Za-z0-9+/=]{45,}\b").unwrap();
    let line = prefixed.replace_all(line, "[redacted]");
    long_runs.replace_all(&line, "[redacted]").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_is_short_sha256_prefix() {
        assert!(fingerprint(b"secret-value").starts_with("sha256:"));
        assert_eq!(fingerprint(b"secret-value").len(), "sha256:12345678".len());
    }

    #[test]
    fn redact_masks_tokens() {
        let raw = "token ghp_abcdefghijklmnop and aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let redacted = redact(raw);
        assert!(!redacted.contains("ghp_"));
        assert!(!redacted.contains("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"));
    }
}
