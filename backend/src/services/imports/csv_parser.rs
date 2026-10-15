//! Shared CSV parsing for the import pipeline.
//!
//! Decodes the file as UTF-8 (BOM-stripped), sniffs the delimiter,
//! reads the header row, then keeps every data row's cells in column
//! order with the file line it starts on. The per-type validator runs
//! over that grid; CSV-level concerns (quoting, escaping, line endings,
//! size caps) end here.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use super::safe_cells::strip_formula_guard;

/// Most data rows one file may carry.
pub const MAX_ROWS: usize = 5_000;
/// Most columns one file may carry.
pub const MAX_COLUMNS: usize = 256;
/// Longest cell, in characters (Excel's own cell limit).
pub const MAX_CELL_CHARS: usize = 32_767;
/// Longest header, in characters.
pub const MAX_HEADER_CHARS: usize = 255;

/// Parsed CSV ready for per-type validation: the header row and each
/// data row's cells in column order, trimmed. Serialized as the job's
/// normalized grid (`mapping::grid_path`).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ParsedCsv {
    pub headers: Vec<String>,
    pub rows: Vec<SourceRow>,
    /// The field separator the header line used.
    pub delimiter: u8,
}

/// One data row of the file.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SourceRow {
    /// 1-based line of the file the row starts on; the header is line 1.
    /// A quoted field spanning lines doesn't shift the rows after it.
    pub source_row: usize,
    /// One cell per header, in header order.
    pub cells: Vec<String>,
}

/// A data row keyed by header, for the importers' by-name lookups.
pub struct Record {
    pub source_row: usize,
    pub values: HashMap<String, String>,
}

impl ParsedCsv {
    /// Every data row keyed by header. Headers are unique, so no cell is
    /// shadowed by another.
    pub fn records(&self) -> impl Iterator<Item = Record> + '_ {
        self.rows.iter().map(|row| Record {
            source_row: row.source_row,
            values: self
                .headers
                .iter()
                .cloned()
                .zip(row.cells.iter().cloned())
                .collect(),
        })
    }
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
    DuplicateHeader(String),
    /// A column with values but no header; 1-based column number.
    BlankHeader(usize),
    HeaderTooLong(usize),
    TooManyColumns(usize),
    TooManyRows,
    CellTooLong {
        row: usize,
        column: String,
    },
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
            Self::DuplicateHeader(name) => write!(
                f,
                "column '{name}' appears more than once in the header row; rename or remove one"
            ),
            Self::BlankHeader(column) => write!(
                f,
                "column {column} has values but no header; name it or delete the column"
            ),
            Self::HeaderTooLong(column) => write!(
                f,
                "the header of column {column} is longer than {MAX_HEADER_CHARS} characters"
            ),
            Self::TooManyColumns(count) => write!(
                f,
                "file has {count} columns; an import takes at most {MAX_COLUMNS}"
            ),
            Self::TooManyRows => write!(
                f,
                "file has more than 5,000 data rows; split it into smaller files"
            ),
            Self::CellTooLong { row, column } => write!(
                f,
                "row {row}, column '{column}' is longer than 32,767 characters"
            ),
        }
    }
}

impl std::error::Error for ParseError {}

/// Parse a CSV file from disk. UTF-8 only; BOM stripped.
pub fn parse_file(path: &Path) -> Result<ParsedCsv, ParseError> {
    let bytes = std::fs::read(path).map_err(ParseError::Io)?;
    parse_bytes(&bytes)
}

/// Parse an uploaded CSV. UTF-8 only; BOM stripped.
pub fn parse_bytes(bytes: &[u8]) -> Result<ParsedCsv, ParseError> {
    let text = strip_bom(bytes)?;
    parse_str(text)
}

fn strip_bom(bytes: &[u8]) -> Result<&str, ParseError> {
    let stripped = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    std::str::from_utf8(stripped).map_err(|_| ParseError::NotUtf8)
}

/// The header line's separator: whichever of `,` `;` and tab occurs most
/// outside quotes, comma on a tie or when there's none.
pub fn sniff_delimiter(text: &str) -> u8 {
    let mut counts = [0usize; 3];
    let mut quoted = false;
    for c in text.chars() {
        match c {
            '"' => quoted = !quoted,
            '\n' | '\r' if !quoted => break,
            ',' if !quoted => counts[0] += 1,
            ';' if !quoted => counts[1] += 1,
            '\t' if !quoted => counts[2] += 1,
            _ => {}
        }
    }
    let mut best = (b',', counts[0]);
    for (delimiter, count) in [(b';', counts[1]), (b'\t', counts[2])] {
        if count > best.1 {
            best = (delimiter, count);
        }
    }
    best.0
}

fn parse_str(text: &str) -> Result<ParsedCsv, ParseError> {
    let delimiter = sniff_delimiter(text);
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(false)
        .delimiter(delimiter)
        .from_reader(text.as_bytes());

    let headers: Vec<String> = reader
        .headers()
        .map_err(ParseError::Csv)?
        .iter()
        .map(|h| h.trim().to_string())
        .collect();
    if headers.iter().all(String::is_empty) {
        return Err(ParseError::MissingHeader);
    }
    if headers.len() > MAX_COLUMNS {
        return Err(ParseError::TooManyColumns(headers.len()));
    }
    let mut seen = HashSet::new();
    for (i, header) in headers.iter().enumerate() {
        if header.chars().count() > MAX_HEADER_CHARS {
            return Err(ParseError::HeaderTooLong(i + 1));
        }
        if !header.is_empty() && !seen.insert(header.as_str()) {
            return Err(ParseError::DuplicateHeader(header.clone()));
        }
    }

    // The reader's own line numbers skip blank lines, so count line
    // breaks up to each record's byte offset instead.
    let mut lines = LineCounter::new(text);
    let mut rows: Vec<SourceRow> = Vec::new();
    for record in reader.records() {
        let record = record.map_err(ParseError::Csv)?;
        if rows.len() == MAX_ROWS {
            return Err(ParseError::TooManyRows);
        }
        let source_row = match record.position() {
            Some(p) => lines.line_at(p.byte() as usize),
            None => rows.len() + 2,
        };
        let mut cells = Vec::with_capacity(headers.len());
        for (i, header) in headers.iter().enumerate() {
            let raw = record.get(i).unwrap_or("");
            if raw.chars().count() > MAX_CELL_CHARS {
                return Err(ParseError::CellTooLong {
                    row: source_row,
                    column: header.clone(),
                });
            }
            cells.push(strip_formula_guard(raw.trim()).trim().to_string());
        }
        rows.push(SourceRow { source_row, cells });
    }

    // A blank header over an empty column (a trailing delimiter from a
    // spreadsheet export) is harmless; one over values is ambiguous.
    for (i, header) in headers.iter().enumerate() {
        if header.is_empty() && rows.iter().any(|r| !r.cells[i].is_empty()) {
            return Err(ParseError::BlankHeader(i + 1));
        }
    }

    Ok(ParsedCsv {
        headers,
        rows,
        delimiter,
    })
}

/// 1-based line of a byte offset, for offsets read in increasing order.
/// `\n`, `\r\n` and a lone `\r` each end a line.
struct LineCounter<'a> {
    text: &'a [u8],
    offset: usize,
    line: usize,
}

impl<'a> LineCounter<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            text: text.as_bytes(),
            offset: 0,
            line: 1,
        }
    }

    /// The line of the first byte at or after `offset` that isn't a line
    /// break: the reader places a record's start before the breaks (blank
    /// lines included) that precede it.
    fn line_at(&mut self, offset: usize) -> usize {
        let end = offset.min(self.text.len());
        while self.offset < end || matches!(self.text.get(self.offset), Some(b'\r' | b'\n')) {
            match self.text[self.offset] {
                b'\n' => self.line += 1,
                b'\r' if self.text.get(self.offset + 1) != Some(&b'\n') => self.line += 1,
                _ => {}
            }
            self.offset += 1;
        }
        self.line
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value<'a>(parsed: &'a ParsedCsv, row: usize, header: &str) -> &'a str {
        let i = parsed.headers.iter().position(|h| h == header).unwrap();
        &parsed.rows[row].cells[i]
    }

    #[test]
    fn strips_bom_and_parses_headers() {
        let parsed = parse_bytes("\u{feff}name,kind\nFoo,generic\n".as_bytes()).unwrap();
        assert_eq!(parsed.headers, vec!["name", "kind"]);
        assert_eq!(parsed.rows.len(), 1);
        assert_eq!(value(&parsed, 0, "name"), "Foo");
    }

    #[test]
    fn trims_field_whitespace() {
        let parsed = parse_str("name,kind\n  Foo  ,  generic  \n").unwrap();
        assert_eq!(value(&parsed, 0, "name"), "Foo");
        assert_eq!(value(&parsed, 0, "kind"), "generic");
    }

    #[test]
    fn empty_data_row_is_just_blank_fields() {
        let parsed = parse_str("name,kind\n,\n").unwrap();
        assert_eq!(parsed.rows.len(), 1);
        assert_eq!(value(&parsed, 0, "name"), "");
    }

    #[test]
    fn rows_carry_their_file_line() {
        let parsed = parse_str(
            "name,notes\r\nA,\"two\r\nlines\"\r\nB,x\r\n\r\nC,\"three\nlines\nhere\"\nD,y\n\n\rE,z\rF,w",
        )
        .unwrap();
        let lines: Vec<usize> = parsed.rows.iter().map(|r| r.source_row).collect();
        assert_eq!(lines, vec![2, 4, 6, 9, 12, 13]);
        assert_eq!(value(&parsed, 0, "notes"), "two\r\nlines");
    }

    #[test]
    fn records_key_cells_by_header() {
        let parsed = parse_str("name,kind\nFoo,generic\n").unwrap();
        let record = parsed.records().next().unwrap();
        assert_eq!(record.source_row, 2);
        assert_eq!(
            record.values.get("kind").map(String::as_str),
            Some("generic")
        );
    }

    #[test]
    fn sniffs_the_delimiter_from_the_header_line() {
        assert_eq!(sniff_delimiter("name,kind,tag\n"), b',');
        assert_eq!(sniff_delimiter("name;kind;tag\n1,5;a;b"), b';');
        assert_eq!(sniff_delimiter("name\tkind\ttag\n"), b'\t');
        // Separators inside quotes don't count.
        assert_eq!(sniff_delimiter("\"a;b;c\",d,e\n"), b',');
        assert_eq!(sniff_delimiter("\"a,b,c\";d;e\n"), b';');
        // Only the header line is read.
        assert_eq!(sniff_delimiter("name\n;;;;\n"), b',');
        // Tie or none: comma.
        assert_eq!(sniff_delimiter("a,b;c\n"), b',');
        assert_eq!(sniff_delimiter("name\n"), b',');
    }

    #[test]
    fn parses_semicolon_and_tab_files() {
        let parsed = parse_str("name;quantity\nToner;12,5\n").unwrap();
        assert_eq!(parsed.delimiter, b';');
        assert_eq!(value(&parsed, 0, "quantity"), "12,5");
        let parsed = parse_str("name\tkind\nFoo\tgeneric\n").unwrap();
        assert_eq!(parsed.delimiter, b'\t');
        assert_eq!(value(&parsed, 0, "kind"), "generic");
    }

    #[test]
    fn strips_the_formula_guard() {
        let parsed = parse_str("name,qty,note\n'=SUM(A1),'-5,'quoted\n").unwrap();
        assert_eq!(value(&parsed, 0, "name"), "=SUM(A1)");
        assert_eq!(value(&parsed, 0, "qty"), "-5");
        assert_eq!(value(&parsed, 0, "note"), "'quoted");
    }

    #[test]
    fn refuses_duplicate_headers() {
        let err = parse_str("name,kind, name \nA,b,c\n").unwrap_err();
        assert!(matches!(&err, ParseError::DuplicateHeader(h) if h == "name"));
        assert!(err.to_string().contains("'name'"), "{err}");
    }

    #[test]
    fn refuses_a_blank_header_over_values_only() {
        let parsed = parse_str("name,kind,\nA,b,\nC,d,\n").unwrap();
        assert_eq!(parsed.headers, vec!["name", "kind", ""]);
        let err = parse_str("name,,kind\nA,x,b\n").unwrap_err();
        assert!(matches!(err, ParseError::BlankHeader(2)));
    }

    #[test]
    fn enforces_the_caps() {
        let wide = (0..=MAX_COLUMNS)
            .map(|i| format!("c{i}"))
            .collect::<Vec<_>>()
            .join(",");
        assert!(matches!(
            parse_str(&format!("{wide}\n")),
            Err(ParseError::TooManyColumns(257))
        ));

        let long_header = "h".repeat(MAX_HEADER_CHARS + 1);
        assert!(matches!(
            parse_str(&format!("name,{long_header}\n")),
            Err(ParseError::HeaderTooLong(2))
        ));

        let long_cell = "x".repeat(MAX_CELL_CHARS + 1);
        let err = parse_str(&format!("name,notes\nA,ok\nB,{long_cell}\n")).unwrap_err();
        assert!(
            matches!(&err, ParseError::CellTooLong { row: 3, column } if column == "notes"),
            "{err:?}"
        );

        let rows = "a\n".repeat(MAX_ROWS);
        assert_eq!(
            parse_str(&format!("name\n{rows}")).unwrap().rows.len(),
            MAX_ROWS
        );
        let err = parse_str(&format!("name\n{rows}a\n")).unwrap_err();
        assert!(matches!(err, ParseError::TooManyRows));
        assert!(err.to_string().contains("5,000"), "{err}");
    }
}
