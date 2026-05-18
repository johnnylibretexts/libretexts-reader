use std::path::Path;

use chrono::Utc;
use once_cell::sync::OnceCell;
use pdfium_render::prelude::{PdfDocumentMetadataTagType, PdfPageTextChar, Pdfium};
use serde_json::json;

use crate::content::document::{DocumentBuilder, SectionBuilder};
use crate::db::models::SourceType;
use crate::error::{AppError, AppResult};

const SCANNED_MIN_CHARS_PER_PAGE: usize = 100;
const SCANNED_DETECTION_PAGES: usize = 5;
const HEADING_FONT_SCALE: f32 = 1.5;
const MAX_HEADING_CHARS: usize = 80;

static PDFIUM: OnceCell<Pdfium> = OnceCell::new();

#[derive(Clone)]
struct ExtractedPage {
    text: String,
    lines: Vec<ExtractedLine>,
}

#[derive(Clone)]
struct ExtractedLine {
    page_index: usize,
    text: String,
    font_size: Option<f32>,
    y: Option<f32>,
    is_first_non_empty_on_page: bool,
}

pub fn import_from_path(path: &Path) -> AppResult<DocumentBuilder> {
    let pdfium = pdfium()?;
    let document = pdfium.load_pdf_from_file(path, None)?;
    let page_count = document.pages().len() as usize;

    if page_count == 0 {
        return Err(AppError::Pdf("PDF does not contain any pages".into()));
    }

    let mut pages = Vec::with_capacity(page_count);
    let mut font_sizes = Vec::new();

    for page_index in document.pages().as_range() {
        let page = document.pages().get(page_index)?;
        let page_text = page.text()?;
        let text = normalize_page_text(&page_text.all());
        let lines = lines_from_page_text(page_index as usize, &page_text, &mut font_sizes);
        pages.push(ExtractedPage { text, lines });
    }

    detect_scanned_pdf(&pages)?;

    let median_font_size = median(font_sizes).unwrap_or(0.0);
    let median_line_gap = median(line_gaps(&pages));
    let sections = build_sections(&pages, median_font_size, median_line_gap)?;
    let title = document_title(path, &document);

    Ok(DocumentBuilder {
        title,
        source_type: SourceType::Pdf,
        source_metadata: json!({
            "file_path": path.to_string_lossy(),
            "page_count": page_count,
            "imported_at": Utc::now().to_rfc3339()
        }),
        cover_image_path: None,
        license: None,
        attribution: None,
        sections,
    })
}

fn pdfium() -> AppResult<&'static Pdfium> {
    PDFIUM.get_or_try_init(|| {
        let bindings = Pdfium::bind_to_library(env!("PDFIUM_LIBRARY_PATH"))
            .map_err(|error| AppError::Pdf(error.to_string()))?;
        Ok(Pdfium::new(bindings))
    })
}

fn document_title(path: &Path, document: &pdfium_render::prelude::PdfDocument<'_>) -> String {
    document
        .metadata()
        .get(PdfDocumentMetadataTagType::Title)
        .map(|tag| tag.value().trim().to_string())
        .filter(|title| !title.is_empty())
        .or_else(|| {
            path.file_stem()
                .and_then(|name| name.to_str())
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| "PDF".to_string())
}

fn lines_from_page_text(
    page_index: usize,
    page_text: &pdfium_render::prelude::PdfPageText<'_>,
    document_font_sizes: &mut Vec<f32>,
) -> Vec<ExtractedLine> {
    let mut lines = Vec::new();
    let mut builder = LineBuilder::new(page_index);

    for character in page_text.chars().iter() {
        let value = character.unicode_char().unwrap_or(' ');

        if value == '\r' {
            continue;
        }

        if value == '\n' {
            lines.push(builder.finish());
            continue;
        }

        let font_size = character.scaled_font_size().value;
        if !value.is_whitespace() && font_size > 0.0 {
            document_font_sizes.push(font_size);
        }

        builder.push(value, &character);
    }

    if !builder.is_empty() {
        lines.push(builder.finish());
    }

    mark_first_non_empty_line(lines)
}

fn mark_first_non_empty_line(mut lines: Vec<ExtractedLine>) -> Vec<ExtractedLine> {
    if let Some(line) = lines.iter_mut().find(|line| !line.text.trim().is_empty()) {
        line.is_first_non_empty_on_page = true;
    }
    lines
}

struct LineBuilder {
    page_index: usize,
    text: String,
    font_sizes: Vec<f32>,
    y_values: Vec<f32>,
}

impl LineBuilder {
    fn new(page_index: usize) -> Self {
        Self {
            page_index,
            text: String::new(),
            font_sizes: Vec::new(),
            y_values: Vec::new(),
        }
    }

    fn push(&mut self, value: char, character: &PdfPageTextChar<'_>) {
        self.text.push(value);

        if !value.is_whitespace() {
            let font_size = character.scaled_font_size().value;
            if font_size > 0.0 {
                self.font_sizes.push(font_size);
            }

            if let Ok(y) = character.origin_y() {
                self.y_values.push(y.value);
            }
        }
    }

    fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    fn finish(&mut self) -> ExtractedLine {
        let text = normalize_line(&self.text);
        let font_size = self.font_sizes.iter().copied().reduce(f32::max);
        let y = median(std::mem::take(&mut self.y_values));

        let line = ExtractedLine {
            page_index: self.page_index,
            text,
            font_size,
            y,
            is_first_non_empty_on_page: false,
        };

        self.text.clear();
        self.font_sizes.clear();
        line
    }
}

fn detect_scanned_pdf(pages: &[ExtractedPage]) -> AppResult<()> {
    let inspected_pages = pages.len().min(SCANNED_DETECTION_PAGES);
    let character_count = pages
        .iter()
        .take(inspected_pages)
        .map(|page| page.text.trim().chars().count())
        .sum::<usize>();

    if character_count < inspected_pages * SCANNED_MIN_CHARS_PER_PAGE {
        return Err(AppError::Pdf(
            "PDF appears to be scanned. OCR is required and not yet supported.".into(),
        ));
    }

    Ok(())
}

fn build_sections(
    pages: &[ExtractedPage],
    median_font_size: f32,
    median_line_gap: Option<f32>,
) -> AppResult<Vec<SectionBuilder>> {
    let mut sections = Vec::new();
    let mut current_title = "Content".to_string();
    let mut current_lines = Vec::new();

    for line in pages.iter().flat_map(|page| page.lines.iter()) {
        if is_heading(line, median_font_size) {
            push_section(
                &mut sections,
                &current_title,
                &current_lines,
                median_line_gap,
            );
            current_title = line.text.clone();
            current_lines.clear();
            continue;
        }

        current_lines.push(line.clone());
    }

    push_section(
        &mut sections,
        &current_title,
        &current_lines,
        median_line_gap,
    );

    if sections.is_empty() {
        let fallback_text = pages
            .iter()
            .map(|page| page.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        let paragraphs = crate::content::split_paragraphs(&fallback_text);

        if !paragraphs.is_empty() {
            sections.push(SectionBuilder {
                title: "Content".to_string(),
                paragraphs,
            });
        }
    }

    if sections.is_empty() {
        Err(AppError::Pdf("PDF did not contain readable text".into()))
    } else {
        Ok(sections)
    }
}

fn push_section(
    sections: &mut Vec<SectionBuilder>,
    title: &str,
    lines: &[ExtractedLine],
    median_line_gap: Option<f32>,
) {
    let paragraphs = paragraphs_from_lines(lines, median_line_gap);
    if !paragraphs.is_empty() {
        sections.push(SectionBuilder {
            title: title.to_string(),
            paragraphs,
        });
    }
}

fn is_heading(line: &ExtractedLine, median_font_size: f32) -> bool {
    let text = line.text.trim();
    if text.is_empty()
        || text.chars().count() > MAX_HEADING_CHARS
        || !line.is_first_non_empty_on_page
        || median_font_size <= 0.0
    {
        return false;
    }

    line.font_size
        .is_some_and(|font_size| font_size > median_font_size * HEADING_FONT_SCALE)
}

fn paragraphs_from_lines(lines: &[ExtractedLine], median_line_gap: Option<f32>) -> Vec<String> {
    let mut paragraphs = Vec::new();
    let mut current = String::new();
    let mut previous_non_empty: Option<&ExtractedLine> = None;

    for line in lines {
        let text = line.text.trim();
        if text.is_empty() {
            push_paragraph(&mut paragraphs, &mut current);
            previous_non_empty = None;
            continue;
        }

        if !current.is_empty()
            && paragraph_gap(previous_non_empty, line, median_line_gap).is_some_and(|gap| gap)
        {
            push_paragraph(&mut paragraphs, &mut current);
        }

        append_line(&mut current, text);
        previous_non_empty = Some(line);
    }

    push_paragraph(&mut paragraphs, &mut current);
    paragraphs
}

fn paragraph_gap(
    previous: Option<&ExtractedLine>,
    current: &ExtractedLine,
    median_line_gap: Option<f32>,
) -> Option<bool> {
    let previous = previous?;
    if previous.page_index != current.page_index {
        return Some(false);
    }

    let gap = (previous.y? - current.y?).abs();
    let median = median_line_gap?;
    Some(median > 0.0 && gap > median * 1.5)
}

fn append_line(current: &mut String, line: &str) {
    if current.is_empty() {
        current.push_str(line);
        return;
    }

    if current.ends_with('-') {
        current.pop();
        current.push_str(line);
    } else {
        current.push(' ');
        current.push_str(line);
    }
}

fn push_paragraph(paragraphs: &mut Vec<String>, current: &mut String) {
    let paragraph = current.split_whitespace().collect::<Vec<_>>().join(" ");
    if !paragraph.is_empty() {
        paragraphs.push(paragraph);
    }
    current.clear();
}

fn line_gaps(pages: &[ExtractedPage]) -> Vec<f32> {
    let mut gaps = Vec::new();

    for page in pages {
        let mut previous_y: Option<f32> = None;

        for line in page
            .lines
            .iter()
            .filter(|line| !line.text.trim().is_empty())
        {
            if let (Some(previous), Some(current)) = (previous_y, line.y) {
                let gap = (previous - current).abs();
                if gap > 0.0 {
                    gaps.push(gap);
                }
            }
            previous_y = line.y;
        }
    }

    gaps
}

fn median(mut values: Vec<f32>) -> Option<f32> {
    values.retain(|value| value.is_finite());
    if values.is_empty() {
        return None;
    }

    values.sort_by(|a, b| a.total_cmp(b));
    Some(values[values.len() / 2])
}

fn normalize_page_text(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn normalize_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
