#[derive(Debug, Clone)]
pub struct CredmanEntry {
    pub target: String,
    pub username: Option<String>,
}

pub fn enumerate(_filter: &str) -> Result<Vec<CredmanEntry>, String> {
    Ok(Vec::new())
}

#[cfg(test)]
mod tests {
    #[test]
    fn module_does_not_reference_secret_read_symbols() {
        let source = include_str!("credman.rs");
        let read_symbol = ["Cred", "Read"].join("");
        let blob_symbol = ["Credential", "Blob"].join("");
        assert!(!source.contains(&read_symbol));
        assert!(!source.contains(&blob_symbol));
    }
}
