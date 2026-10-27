//! The conversion report: one row per page with its path, title, outcome
//! and every note, for the admin to read or keep.

use super::{DocsRows, ImageProblem, PageOutcome, PageRow};
use crate::services::docs_markdown::NoteCode;
use crate::services::imports::safe_cells::csv_formula_safe;

const COLUMNS: [&str; 4] = ["Path", "Title", "Outcome", "Notes"];

/// A note as the report words it, with its count.
pub fn note_text(code: NoteCode, count: u32) -> String {
    let what = match code {
        NoteCode::TablesHoisted => "tables moved out of a list or quote",
        NoteCode::TableRowsPadded => "table rows evened out",
        NoteCode::TableCellsFlattened => "table cells reduced to paragraphs",
        NoteCode::ChecklistsAsBullets => "checklists became bullet lists",
        NoteCode::StrikethroughRemoved => "strikethrough or underline removed",
        NoteCode::CalloutsAsQuotes => "callouts became quotes",
        NoteCode::FormulasAsCode => "formulas kept as code",
        NoteCode::LinksDropped => "links that can't be kept, text kept",
        NoteCode::ImagesDropped => "images that can't be shown, alt text kept",
        NoteCode::HtmlRemoved => "embedded HTML removed",
        NoteCode::HtmlAsText => "HTML kept as its text",
        NoteCode::NestingFlattened => "nesting flattened",
        NoteCode::TableCellsDropped => "extra table cells dropped",
        NoteCode::MentionsAsText => "mentions became text",
        NoteCode::TicketLinksAsText => "ticket links became text",
        NoteCode::EmbedsAsText => "embedded pages became their title",
        NoteCode::LinksUnresolved => "links to files that aren't in the import, text kept",
        NoteCode::LinksAmbiguous => "links by a name more than one page shares",
        NoteCode::AttachmentsLeftOut => "attachments left out",
        NoteCode::FrontMatterIgnored => "front matter couldn't be read",
        NoteCode::TitleShortened => "title shortened",
        NoteCode::NotUtf8 => "file wasn't UTF-8",
        NoteCode::SourceSyntaxRemoved => "tool syntax removed",
        NoteCode::DatabaseRowsCut => "database rows cut to 200",
        NoteCode::ImagesUnreadable => "images couldn't be read",
        NoteCode::SvgImagesRefused => "SVG images left out",
        NoteCode::WebImagesNotFetched => "web images couldn't be downloaded",
    };
    format!("{what} ({count})")
}

fn outcome_text(outcome: PageOutcome) -> &'static str {
    match outcome {
        PageOutcome::New => "New",
        PageOutcome::Skipped => "Skipped, already in Nosdesk",
        PageOutcome::Error => "Imported empty, the file couldn't be read",
    }
}

/// Every note on a page as one cell: the conversion's notes, each image
/// left out and why, the links that point nowhere and the attachments.
pub fn notes_cell(row: &PageRow) -> String {
    let mut parts: Vec<String> = row.notes.iter().map(|(c, n)| note_text(c, n)).collect();
    if let Some(error) = &row.error {
        parts.push(error.clone());
    }
    for image in &row.images {
        if let Some(problem) = image.problem {
            parts.push(format!("Image {} {}", image.source, problem_text(problem)));
        }
    }
    for link in &row.links.unresolved {
        parts.push(format!("Link to {link} isn't in the import"));
    }
    for file in &row.links.attachments {
        parts.push(format!("Attachment {file} left out"));
    }
    parts.join("; ")
}

fn problem_text(problem: ImageProblem) -> &'static str {
    problem.text()
}

/// The report as CSV, with a BOM so spreadsheet apps read it as UTF-8.
pub fn report_csv(rows: &DocsRows) -> Result<Vec<u8>, csv::Error> {
    let mut buf = "\u{FEFF}".as_bytes().to_vec();
    {
        let mut wtr = csv::Writer::from_writer(&mut buf);
        wtr.write_record(COLUMNS)?;
        for row in &rows.pages {
            wtr.write_record(
                [
                    row.path.clone(),
                    row.title.clone(),
                    outcome_text(row.outcome).to_string(),
                    notes_cell(row),
                ]
                .into_iter()
                .map(csv_formula_safe),
            )?;
        }
        wtr.flush()?;
    }
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::docs_markdown::links::LinkReport;
    use crate::services::docs_markdown::Notes;
    use crate::services::imports::documentation::{ImageKind, PageImage};

    #[test]
    fn a_page_lists_every_note_and_formulas_are_neutralised() {
        let mut notes = Notes::default();
        notes.add(NoteCode::TablesHoisted);
        notes.add(NoteCode::TablesHoisted);
        let mut links = LinkReport::default();
        links.unresolved.insert("old/setup.md".into());
        let rows = DocsRows {
            kind: "documentation".into(),
            pages: vec![PageRow {
                index: 0,
                path: "guide/vpn.md".into(),
                title: "=HYPERLINK(\"x\")".into(),
                parent: None,
                outcome: PageOutcome::New,
                duplicate: None,
                source_uuid: None,
                notes,
                links,
                images: vec![PageImage {
                    source: "img/chart.svg".into(),
                    kind: ImageKind::Archive,
                    name: "chart.svg".into(),
                    size: None,
                    problem: Some(ImageProblem::Svg),
                }],
                error: None,
            }],
            web_images: Default::default(),
        };
        let csv = String::from_utf8(report_csv(&rows).unwrap()).unwrap();
        assert!(
            csv.starts_with("\u{FEFF}Path,Title,Outcome,Notes\n"),
            "{csv}"
        );
        assert!(csv.contains("guide/vpn.md"), "{csv}");
        assert!(csv.contains("'=HYPERLINK"), "{csv}");
        assert!(
            csv.contains("tables moved out of a list or quote (2)"),
            "{csv}"
        );
        assert!(
            csv.contains("Image img/chart.svg SVG images can't be imported"),
            "{csv}"
        );
        assert!(
            csv.contains("Link to old/setup.md isn't in the import"),
            "{csv}"
        );
    }
}
