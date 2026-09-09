use regex::Regex;
use sha2::{Digest, Sha256};
use std::sync::OnceLock;

pub fn fingerprint(secret: &[u8]) -> String {
    let digest = Sha256::digest(secret);
    format!(
        "sha256:{:02x}{:02x}{:02x}{:02x}",
        digest[0], digest[1], digest[2], digest[3]
    )
}

pub fn redact(line: &str) -> String {
    static PREFIXED: OnceLock<Regex> = OnceLock::new();
    static LONG_RUNS: OnceLock<Regex> = OnceLock::new();
    let prefixed =
        PREFIXED.get_or_init(|| Regex::new(r"\b(ghp|gho|github_pat)_[A-Za-z0-9_]{12,}\b").unwrap());
    let long_runs = LONG_RUNS
        .get_or_init(|| Regex::new(r"\b[A-Fa-f0-9]{40,}\b|\b[A-Za-z0-9+/=]{45,}\b").unwrap());
    let line = prefixed.replace_all(line, "[redacted]");
    long_runs.replace_all(&line, "[redacted]").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprints_keep_the_existing_short_sha256_representation() {
        assert_eq!(fingerprint(b"abc"), "sha256:ba7816bf");
        assert_eq!(fingerprint(b""), "sha256:e3b0c442");
    }

    #[test]
    fn cached_redactors_keep_calls_independent() {
        for (input, expected) in [
            ("ghp_abcdefghijklmnop", "[redacted]"),
            ("fixture@example.test", "fixture@example.test"),
            ("github_pat_ABCDEFGHIJKLMNOPQRSTUV", "[redacted]"),
            (".config/gh/hosts.yml", ".config/gh/hosts.yml"),
        ] {
            assert_eq!(redact(input), expected);
        }
    }

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
