use ini::Ini;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::fs;
use std::io::Read;
use std::path::Path;

const MAX_PARSE_BYTES: u64 = 1_048_576;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Json,
    Yaml,
    Ini,
    Toml,
}

#[derive(Debug, Clone)]
pub struct ParsedDoc {
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub code: String,
    pub message: String,
}

pub(crate) fn read_regular_text(path: &Path) -> Result<String, ParseError> {
    let io_error = |error: std::io::Error| ParseError {
        code: "io_error".into(),
        message: error.to_string(),
    };
    if !fs::metadata(path).map_err(io_error)?.is_file() {
        return Err(ParseError {
            code: "not_regular_file".into(),
            message: "The source is not a regular file".into(),
        });
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options.open(path).map_err(io_error)?;
    let metadata = file.metadata().map_err(io_error)?;
    if !metadata.is_file() {
        return Err(ParseError {
            code: "not_regular_file".into(),
            message: "The source is not a regular file".into(),
        });
    }
    if metadata.len() > MAX_PARSE_BYTES {
        return Err(ParseError {
            code: "file_too_large".into(),
            message: "The source exceeds the 1 MB parser cap".into(),
        });
    }
    let mut text = String::new();
    file.take(MAX_PARSE_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(io_error)?;
    if text.len() as u64 > MAX_PARSE_BYTES {
        return Err(ParseError {
            code: "file_too_large".into(),
            message: "The source exceeds the 1 MB parser cap".into(),
        });
    }
    Ok(text)
}

pub fn parse(path: &Path, format: Format) -> Result<ParsedDoc, ParseError> {
    parse_text(&read_regular_text(path)?, format)
}

pub(crate) fn parse_text(text: &str, format: Format) -> Result<ParsedDoc, ParseError> {
    if text.len() as u64 > MAX_PARSE_BYTES {
        return Err(ParseError {
            code: "file_too_large".into(),
            message: "The source exceeds the 1 MB parser cap".into(),
        });
    }
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);

    let value = match format {
        Format::Json => serde_json::from_str(text).map_err(|err| ParseError {
            code: "parse_error".to_string(),
            message: err.to_string(),
        })?,
        Format::Yaml => serde_yaml::from_str(text).map_err(|err| ParseError {
            code: "parse_error".to_string(),
            message: err
                .location()
                .map(|location| {
                    format!(
                        "Invalid YAML at line {}, column {}",
                        location.line(),
                        location.column()
                    )
                })
                .unwrap_or_else(|| "Invalid YAML document".to_string()),
        })?,
        Format::Ini => ini_to_value(text)?,
        Format::Toml => toml_to_value(text)?,
    };

    Ok(ParsedDoc { value })
}

fn toml_to_value(text: &str) -> Result<Value, ParseError> {
    let value = toml::from_str::<toml::Value>(text).map_err(|err| ParseError {
        code: "parse_error".to_string(),
        message: err
            .span()
            .map(|span| format!("Invalid TOML at byte {}", span.start))
            .unwrap_or_else(|| "Invalid TOML document".to_string()),
    })?;
    serde_json::to_value(value).map_err(|err| ParseError {
        code: "parse_error".to_string(),
        message: err.to_string(),
    })
}

fn ini_to_value(text: &str) -> Result<Value, ParseError> {
    let ini = Ini::load_from_str(text).map_err(|_| ParseError {
        code: "parse_error".to_string(),
        message: "Invalid INI document".to_string(),
    })?;
    let mut root = Map::new();
    for (section, props) in ini.iter() {
        let mut values = Map::new();
        for (key, value) in props.iter() {
            values.insert(key.to_string(), Value::String(value.to_string()));
        }
        root.insert(
            section.unwrap_or("default").to_string(),
            Value::Object(values),
        );
    }
    Ok(Value::Object(root))
}

impl ParsedDoc {
    pub fn select(&self, selector: &str) -> Vec<Value> {
        let mut current = vec![&self.value];
        for part in selector.split('.') {
            let mut next = Vec::new();
            for value in current {
                match (part, value) {
                    ("*", Value::Object(map)) => next.extend(map.values()),
                    ("*", Value::Array(values)) => next.extend(values.iter()),
                    (key, Value::Object(map)) => {
                        if let Some(value) = map.get(key) {
                            next.push(value);
                        }
                    }
                    _ => {}
                }
            }
            current = next;
        }
        current.into_iter().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_rejects_nonregular_sources_and_oversized_text() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            parse(dir.path(), Format::Json).unwrap_err().code,
            "not_regular_file"
        );
        assert_eq!(
            parse_text(&" ".repeat(MAX_PARSE_BYTES as usize + 1), Format::Json)
                .unwrap_err()
                .code,
            "file_too_large"
        );
    }

    #[cfg(unix)]
    #[test]
    fn parser_rejects_fifo_without_waiting_for_a_writer() {
        use std::os::unix::ffi::OsStrExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fixture.fifo");
        let c_path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);
        assert_eq!(
            parse(&path, Format::Json).unwrap_err().code,
            "not_regular_file"
        );
    }

    #[test]
    fn parser_errors_do_not_include_credential_lines() {
        let dir = tempfile::tempdir().unwrap();
        for (format, content) in [
            (Format::Toml, "token = fixture-sensitive-value"),
            (Format::Ini, "[fixture-sensitive-value"),
            (Format::Yaml, "token: [fixture-sensitive-value"),
        ] {
            let path = dir.path().join("invalid-config");
            fs::write(&path, content).unwrap();
            let error = parse(&path, format).unwrap_err();
            assert!(!error.message.contains("fixture-sensitive-value"));
            assert_eq!(error.code, "parse_error");
        }
    }

    #[test]
    fn selector_supports_wildcards() {
        let doc = ParsedDoc {
            value: serde_json::json!({"hosts":{"github.com":{"users":{"alice":{"token":"x"},"bob":{"token":"y"}}}}}),
        };
        assert_eq!(doc.select("hosts.*.users.*").len(), 2);
    }

    #[test]
    fn selector_keeps_nested_results_and_order_without_copying_unselected_branches() {
        let doc = ParsedDoc {
            value: serde_json::json!({"profiles":[{"user":{"name":"Alice"}},{"user":{"name":"Bob"}}],"unrelated":{"token":"fixture-secret"}}),
        };
        assert_eq!(
            doc.select("profiles.*.user.name"),
            vec![Value::String("Alice".into()), Value::String("Bob".into())]
        );
        assert_eq!(
            doc.select("profiles.*.user"),
            vec![
                serde_json::json!({"name":"Alice"}),
                serde_json::json!({"name":"Bob"})
            ]
        );
        assert!(doc.select("profiles.*.missing").is_empty());
    }

    #[test]
    fn json_parser_allows_utf8_bom() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profile.json");
        fs::write(&path, "\u{feff}{\"subscriptions\":[]}").unwrap();

        let doc = parse(&path, Format::Json).unwrap();

        assert!(doc.value.get("subscriptions").is_some());
    }
}
