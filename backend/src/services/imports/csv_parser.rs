//! Shared CSV parsing for the import pipeline.
//!
//! Decodes the file as UTF-8 (BOM-stripped), reads the header
//! row, then collects every data row as a name->value map. The
//! per-type validator runs over that structure; CSV-level
//! concerns (quoting, escaping, line endings) end here.

use std::collections::HashMap;
use std::path::Path;

/// Parsed CSV ready for per-type validation. Header order is
/// preserved so the validator can complain about the right
/// columns by name, but lookups go through a per-row map for
/// O(1) field access.
#[derive(Debug, Clone)]
pub struct ParsedCsv {
    pub headers: Vec<String>,
    pub rows: Vec<HashMap<String, String>>,
}

#[derive(Debug)]
pub enum ParseError {
    Io(std::io::Error),
    Csv(csv::Error),
    /// Backup files frequently arrive as UTF-16 from
    /// Windows-side spreadsheet exports. We don't decode them
    /// silently because the user almost always wants to know
    /// before half their non-ASCII data ends up mojibake.
    NotUtf8,
    /// First non-blank line must be a header row.
    MissingHeader,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Csv(e) => write!(f, "CSV parse error: {e}"),
            Self::NotUtf8 => write!(
                f,
                "file is not valid UTF-8; re-save as UTF-8 (with or without BOM) and retry"
            ),
            Self::MissingHeader => write!(f, "file has no header row"),
        }
    }
}

impl std::error::Error for ParseError {}

/// Parse a CSV file from disk. UTF-8 only; BOM stripped.
pub fn parse_file(path: &Path) -> Result<ParsedCsv, ParseError> {
    let bytes = std::fs::read(path).map_err(ParseError::Io)?;
    let text = strip_bom(&bytes)?;
    parse_str(text)
}

fn strip_bom(bytes: &[u8]) -> Result<&str, ParseError> {
    let stripped = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    std::str::from_utf8(stripped).map_err(|_| ParseError::NotUtf8)
}

fn parse_str(text: &str) -> Result<ParsedCsv, ParseError> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(false)
        .from_reader(text.as_bytes());

    let headers: Vec<String> = reader
        .headers()
        .map_err(ParseError::Csv)?
        .iter()
        .map(|h| h.trim().to_string())
        .collect();
    if headers.is_empty() {
        return Err(ParseError::MissingHeader);
    }

    let mut rows: Vec<HashMap<String, String>> = Vec::new();
    for record in reader.records() {
        let record = record.map_err(ParseError::Csv)?;
        let mut row = HashMap::with_capacity(headers.len());
        for (i, header) in headers.iter().enumerate() {
            let value = record.get(i).unwrap_or("").trim().to_string();
            row.insert(header.clone(), value);
        }
        rows.push(row);
    }

    Ok(ParsedCsv { headers, rows })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_bom_and_parses_headers() {
        let text = "\u{feff}name,kind\nFoo,generic\n";
        let parsed = parse_str(&text[3..]).unwrap();
        assert_eq!(parsed.headers, vec!["name", "kind"]);
        assert_eq!(parsed.rows.len(), 1);
        assert_eq!(parsed.rows[0].get("name").map(String::as_str), Some("Foo"));
    }

    #[test]
    fn trims_field_whitespace() {
        let parsed = parse_str("name,kind\n  Foo  ,  generic  \n").unwrap();
        assert_eq!(parsed.rows[0].get("name").map(String::as_str), Some("Foo"));
        assert_eq!(
            parsed.rows[0].get("kind").map(String::as_str),
            Some("generic")
        );
    }

    #[test]
    fn empty_data_row_is_just_blank_fields() {
        let parsed = parse_str("name,kind\n,\n").unwrap();
        assert_eq!(parsed.rows.len(), 1);
        assert_eq!(parsed.rows[0].get("name").map(String::as_str), Some(""));
    }
}
