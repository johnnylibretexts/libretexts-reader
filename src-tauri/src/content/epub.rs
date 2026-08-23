use std::fs::File;
use std::io::Read;
use std::path::Path;

use chrono::Utc;
use rbook::Epub;
use scraper::{ElementRef, Html, Selector};
use serde_json::json;
use uuid::Uuid;
use zip::ZipArchive;

use crate::content::document::{DocumentBuilder, SectionBuilder};
use crate::content::html_section::{self, SectionSource};
use crate::content::split_paragraphs;
use crate::db::models::SourceType;
use crate::error::{AppError, AppResult};
use crate::paths;

pub fn import_from_path(path: &Path) -> AppResult<DocumentBuilder> {
    import_from_path_in(path, &paths::covers_dir()?)
}

/// The covers directory is a parameter so a test can pass a temporary one.
///
/// `paths.rs` creates every directory it resolves, so calling
/// `paths::covers_dir()` in a test writes into the real application-data tree
/// and is indistinguishable from a reader importing a book. Same shape as
/// `cache::cache_path_in` and `cleanup::reclaim_in`, and the reason the EPUB
/// fixture had to be built without a cover image until now.
pub fn import_from_path_in(path: &Path, covers_dir: &Path) -> AppResult<DocumentBuilder> {
    detect_drm(path)?;

    let book = Epub::open(path).map_err(|error| AppError::Epub(error.to_string()))?;
    let title = book
        .metadata()
        .title()
        .map(|title| title.value().to_string())
        .filter(|title| !title.trim().is_empty())
        .unwrap_or_else(|| {
            path.file_stem()
                .and_then(|name| name.to_str())
                .map_or_else(|| "EPUB".to_string(), ToOwned::to_owned)
        });
    let mut sections = Vec::new();
    let mut unreadable = 0usize;

    // The reader walks the spine -- the publisher's reading order -- rather
    // than the manifest or the archive, either of which can be in any order at
    // all. A chapter that fails to read is skipped rather than failing the
    // import: one unreadable file should not cost the reader the other three
    // hundred.
    for (index, chapter) in book.reader().enumerate() {
        let chapter = match chapter {
            Ok(chapter) => chapter,
            Err(error) => {
                // Skipping is right -- one unreadable file should not cost the
                // reader the other three hundred -- but it used to leave no
                // trace at all, so a book that imported 3 of 300 chapters was
                // indistinguishable from a 3-chapter book.
                unreadable += 1;
                tracing::warn!(
                    spine_index = index,
                    %error,
                    "skipping an EPUB chapter that could not be read"
                );
                continue;
            }
        };

        let media_type = chapter.manifest_entry().media_type();
        if !media_type.contains("html") && !media_type.contains("xml") {
            continue;
        }

        if let Some(section) = section_from_html(chapter.content(), index) {
            sections.push(section);
        }
    }

    if sections.is_empty() {
        // Two different failures wearing one message until now. An archive
        // whose every chapter failed to open is not a book with no text in it,
        // and telling the reader the latter sends them looking for an
        // extraction bug in files that never opened.
        return Err(AppError::InvalidInput(if unreadable > 0 {
            format!(
                "None of this EPUB's {unreadable} chapters could be read. The file may be damaged."
            )
        } else {
            "EPUB did not contain readable text".to_string()
        }));
    }

    if unreadable > 0 {
        tracing::warn!(
            unreadable,
            imported = sections.len(),
            "imported an EPUB with unreadable chapters"
        );
    }

    // Only now that the import will succeed. Writing the cover first left a
    // {uuid}.{ext} file behind on every rejected import -- unreferenceable by
    // construction, and cleanup.rs never touches covers.
    let cover_image_path = save_cover(&book, covers_dir)?;

    Ok(DocumentBuilder {
        title,
        source_type: SourceType::Epub,
        source_metadata: json!({
            "file_path": path.to_string_lossy(),
            "imported_at": Utc::now().to_rfc3339()
        }),
        cover_image_path,
        license: None,
        attribution: None,
        sections,
    })
}

fn detect_drm(path: &Path) -> AppResult<()> {
    let file = File::open(path)?;
    let mut archive = ZipArchive::new(file)
        .map_err(|error| AppError::InvalidInput(format!("invalid EPUB archive: {error}")))?;

    let Ok(mut encryption) = archive.by_name("META-INF/encryption.xml") else {
        return Ok(());
    };

    let mut content = String::new();
    encryption.read_to_string(&mut content)?;
    if content.contains("http://www.adobe.com/adept") || content.contains("<EncryptedKey") {
        return Err(AppError::DrmProtected);
    }

    Ok(())
}

fn save_cover(book: &Epub, covers_dir: &Path) -> AppResult<Option<String>> {
    let Some(cover) = book.manifest().cover_image() else {
        return Ok(None);
    };
    let mime = cover.media_type().to_string();
    let Ok(bytes) = cover.read_bytes() else {
        // A cover that will not read is not worth failing an import over: the
        // Library card already falls back to the Source icon for a book with
        // no cover at all.
        return Ok(None);
    };

    let extension = match mime.as_str() {
        "image/jpeg" | "image/jpg" => "jpg",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        _ => "bin",
    };
    let path = covers_dir.join(format!("{}.{}", Uuid::new_v4(), extension));
    std::fs::write(&path, bytes)?;
    Ok(Some(path.to_string_lossy().to_string()))
}

/// EPUB carries no source-specific chrome the way a MindTouch or OpenStax page
/// does, so it keeps everything the reader finds. Adopting the shared reader is
/// what gives EPUB imports MathML tokens, which its own parser never produced.
struct EpubSource;

impl SectionSource for EpubSource {
    fn should_skip_paragraph(&self, _element: &ElementRef<'_>) -> bool {
        false
    }
}

fn section_from_html(html: &str, index: usize) -> Option<SectionBuilder> {
    let document = Html::parse_document(html);
    let heading_selector = Selector::parse("h1, h2, h3").expect("valid heading selector");
    let title = document
        .select(&heading_selector)
        .find_map(|element| normalized_text(element.text()))
        .unwrap_or_else(|| format!("Section {}", index + 1));

    // Images are deliberately dropped here: an EPUB's are zip entries, not
    // URLs, so the shared downloader has nothing to fetch.
    let paragraphs: Vec<String> = html_section::paragraphs_from_html(html, &EpubSource)
        .into_iter()
        .filter(|text| *text != title)
        .flat_map(|text| split_paragraphs(&text))
        .collect();

    if paragraphs.is_empty() {
        None
    } else {
        Some(SectionBuilder::text(title, paragraphs))
    }
}

fn normalized_text<'a>(text: impl Iterator<Item = &'a str>) -> Option<String> {
    let value = text.collect::<Vec<_>>().join(" ");
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Import against a throwaway covers directory.
    ///
    /// `import_from_path` resolves `paths::covers_dir()`, and `paths.rs`
    /// creates every directory it resolves -- so a test calling it writes into
    /// the real application-data tree, which `check-app-data-isolation.sh`
    /// fails the build over. Every test here goes through this instead.
    fn import_isolated(path: &Path) -> AppResult<DocumentBuilder> {
        let covers = tempfile::tempdir().expect("temp covers dir");
        import_from_path_in(path, covers.path())
    }

    /// The same book with both spine entries missing from the archive, so
    /// every chapter read fails while the container and OPF parse fine.
    fn epub_with_unreadable_chapters(directory: &Path) -> std::path::PathBuf {
        use std::io::Write;
        use zip::write::SimpleFileOptions;

        let path = directory.join("unreadable.epub");
        let mut zip = zip::ZipWriter::new(File::create(&path).expect("create the fixture"));
        let stored =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        zip.start_file("mimetype", stored).expect("mimetype entry");
        zip.write_all(b"application/epub+zip")
            .expect("mimetype body");

        let deflated = SimpleFileOptions::default();
        zip.start_file("META-INF/container.xml", deflated)
            .expect("container entry");
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#,
        )
        .expect("container body");

        zip.start_file("OEBPS/content.opf", deflated)
            .expect("opf entry");
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="id">urn:uuid:fixture</dc:identifier>
    <dc:title>A Fixture Book</dc:title>
    <dc:language>en</dc:language>
  </metadata>
  <manifest>
    <item id="cover" href="cover.png" media-type="image/png" properties="cover-image"/>
    <item id="c1" href="chapter1.xhtml" media-type="application/xhtml+xml"/>
    <item id="c2" href="chapter2.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="c1"/>
    <itemref idref="c2"/>
  </spine>
</package>"#,
        )
        .expect("opf body");

        // A readable cover on an otherwise unreadable book. Without it the
        // orphan test is vacuous: save_cover returns None the moment the
        // manifest names no cover, so it writes nothing whatever order it
        // runs in, and the test passes against the bug it exists to catch.
        zip.start_file("OEBPS/cover.png", deflated)
            .expect("cover entry");
        zip.write_all(&[
            0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, b'I', b'H',
            b'D', b'R',
        ])
        .expect("cover body");

        // The two chapter files are deliberately never written.
        zip.finish().expect("finish the fixture");
        path
    }

    #[test]
    fn an_epub_whose_chapters_all_fail_says_so() {
        // Every spine entry failing used to fall through to the empty-sections
        // guard and report "did not contain readable text" -- which describes
        // a different problem than the one that happened, and sends the reader
        // looking for an extraction bug in files that never opened.
        let directory = tempfile::tempdir().expect("temp dir");
        let path = epub_with_unreadable_chapters(directory.path());

        let message = match import_isolated(&path) {
            Err(error) => error.to_string(),
            Ok(_) => panic!("an archive with no readable chapter must not import"),
        };

        assert!(
            message.contains("2"),
            "the message should say how many chapters could not be read: {message}"
        );
        assert!(
            !message.contains("did not contain readable text"),
            "an unreadable archive is not an empty one: {message}"
        );
    }

    #[test]
    fn a_rejected_import_leaves_no_cover_behind() {
        // save_cover ran before the empty-sections guard, so a rejected import
        // wrote a {uuid}.{ext} file into covers_dir that nothing can ever
        // reference -- and cleanup.rs reaps Kokoro artifacts and stale TTS
        // audio, never covers.
        let directory = tempfile::tempdir().expect("temp dir");
        let covers = directory.path().join("covers");
        std::fs::create_dir_all(&covers).expect("covers dir");
        let path = epub_with_unreadable_chapters(directory.path());

        let _ = import_from_path_in(&path, &covers);

        let leftovers: Vec<_> = std::fs::read_dir(&covers)
            .expect("read covers")
            .filter_map(Result::ok)
            .collect();
        assert!(
            leftovers.is_empty(),
            "a failed import left {} orphaned file(s) nothing reaps",
            leftovers.len()
        );
    }

    /// Nothing in this repo has ever opened a real EPUB in a test: the cases
    /// above all start from an HTML string, so the crate boundary -- the zip,
    /// the container, the OPF, the spine -- was covered by nothing at all.
    ///
    /// Deliberately has no cover image. `save_cover` resolves
    /// `paths::covers_dir()`, and `paths.rs` creates every directory it
    /// resolves, so a fixture with a cover would write into the real
    /// application-data tree and be indistinguishable from a reader importing
    /// a book.
    fn minimal_epub(directory: &Path) -> std::path::PathBuf {
        use std::io::Write;
        use zip::write::SimpleFileOptions;

        let path = directory.join("fixture.epub");
        let mut zip = zip::ZipWriter::new(File::create(&path).expect("create the fixture"));
        let stored =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

        // Uncompressed and first, as the specification requires.
        zip.start_file("mimetype", stored).expect("mimetype entry");
        zip.write_all(b"application/epub+zip")
            .expect("mimetype body");

        let deflated = SimpleFileOptions::default();
        zip.start_file("META-INF/container.xml", deflated)
            .expect("container entry");
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#,
        )
        .expect("container body");

        zip.start_file("OEBPS/content.opf", deflated)
            .expect("opf entry");
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="id">urn:uuid:fixture</dc:identifier>
    <dc:title>A Fixture Book</dc:title>
    <dc:language>en</dc:language>
  </metadata>
  <manifest>
    <item id="c1" href="chapter1.xhtml" media-type="application/xhtml+xml"/>
    <item id="c2" href="chapter2.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="c1"/>
    <itemref idref="c2"/>
  </spine>
</package>"#,
        )
        .expect("opf body");

        zip.start_file("OEBPS/chapter1.xhtml", deflated)
            .expect("chapter one entry");
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"><body>
  <h1>Chapter One</h1>
  <p>The first paragraph of the book.</p>
  <p>Given <math xmlns="http://www.w3.org/1998/Math/MathML"><mi>x</mi></math> we continue.</p>
</body></html>"#,
        )
        .expect("chapter one body");

        zip.start_file("OEBPS/chapter2.xhtml", deflated)
            .expect("chapter two entry");
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"><body>
  <h1>Chapter Two</h1>
  <p>The second chapter says something else.</p>
</body></html>"#,
        )
        .expect("chapter two body");

        zip.finish().expect("finish the fixture");
        path
    }

    #[test]
    fn imports_a_real_epub_file_end_to_end() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let path = minimal_epub(directory.path());

        let document = import_isolated(&path).expect("the fixture should import");

        assert_eq!(document.title, "A Fixture Book");
        assert_eq!(document.sections.len(), 2);
        assert_eq!(document.sections[0].title, "Chapter One");
        assert_eq!(document.sections[1].title, "Chapter Two");
        assert!(document.cover_image_path.is_none());
    }

    #[test]
    fn keeps_the_spine_order_rather_than_the_archive_order() {
        // The zip happens to store the chapters in reading order, so this
        // would pass on an accident. It is here because the reading order is
        // the spine's to decide, and a replacement that iterated the manifest
        // or the archive would only be caught by an assertion that names it.
        let directory = tempfile::tempdir().expect("a temporary directory");
        let path = minimal_epub(directory.path());

        let document = import_isolated(&path).expect("the fixture should import");

        let titles: Vec<&str> = document
            .sections
            .iter()
            .map(|section| section.title.as_str())
            .collect();
        assert_eq!(titles, vec!["Chapter One", "Chapter Two"]);
    }

    #[test]
    fn a_real_epub_keeps_its_mathml_through_the_archive() {
        // The HTML-level version of this above proves the shared reader keeps
        // math. This proves it survives the whole path: zip, container, OPF,
        // spine, and whatever the EPUB crate hands back. A crate that returned
        // stripped text instead of markup would pass every other test here.
        let directory = tempfile::tempdir().expect("a temporary directory");
        let path = minimal_epub(directory.path());

        let document = import_isolated(&path).expect("the fixture should import");

        assert!(
            document.sections[0]
                .paragraphs
                .iter()
                .any(|paragraph| paragraph.contains("[[mathml:")),
            "{:?}",
            document.sections[0].paragraphs
        );
    }

    #[test]
    fn reads_a_chapter_into_paragraphs() {
        let section = section_from_html(
            "<h1>Chapter One</h1><p>First paragraph.</p><p>Second paragraph.</p>",
            0,
        )
        .expect("a section");

        assert_eq!(section.title, "Chapter One");
        assert_eq!(section.paragraphs.len(), 2);
    }

    #[test]
    fn now_preserves_mathml_the_way_the_other_importers_do() {
        // EPUB's own parser dropped math into bare glyphs. Adopting the shared
        // reader is what gives it tokens the reader can render with KaTeX.
        let section = section_from_html(
            "<h1>Maths</h1><p>Given <math><mi>x</mi></math> we continue.</p>",
            0,
        )
        .expect("a section");

        assert!(
            section.paragraphs.iter().any(|p| p.contains("[[mathml:")),
            "{:?}",
            section.paragraphs
        );
    }

    #[test]
    fn a_section_with_no_readable_text_is_dropped() {
        assert!(section_from_html("<div></div>", 0).is_none());
    }
}
