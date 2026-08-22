//! ID3 tags for an exported chapter.
//!
//! A chapter MP3 leaves the app as a derivative work, and until this existed it
//! left with the credit stripped: licence and attribution were captured at
//! import and then written nowhere. CC BY 4.0 §3(a)(1) attaches to the copy the
//! reader is holding, and a file handed to someone else is exactly that copy.

use std::path::Path;

use id3::frame::Content;
use id3::{Frame, Tag, TagLike, Version};

use crate::db::models::{Document, Section};
use crate::error::{AppError, AppResult};

/// Write the chapter's identity, licence and credit into the file's ID3 tags.
///
/// Tags the file rather than the bytes, because Fish returns MP3 data that may
/// already carry a tag of its own and prepending a second one is not the same
/// as replacing it. `id3` handles that; hand-rolling the syncsafe header would
/// be the kind of code that is subtly wrong for years.
pub(crate) fn tag_chapter_mp3(
    path: &Path,
    document: &Document,
    section: &Section,
) -> AppResult<()> {
    let mut tag = Tag::new();

    // Always known, so the tag is never empty even for a Source that supplied
    // no licence at all.
    tag.set_album(&document.title);
    tag.set_title(&section.title);

    if let Some(license) = present(&document.license) {
        tag.add_frame(Frame::text("TCOP", license));
    }

    if let Some(attribution) = present(&document.attribution) {
        // `documents.attribution` is polymorphic by Source: OpenStax,
        // LibreTexts and article store a URL, Pressbooks stores an author
        // name. Written as the artist, a URL fills every music player's artist
        // column with "https://openstax.org/..."; written as a link, an author
        // is not a link at all. So the shape decides the frame.
        match source_url(attribution) {
            Some(url) => tag.add_frame(Frame::with_content("WOAS", Content::Link(url))),
            None => {
                tag.set_artist(attribution);
                None
            }
        };
    }

    tag.write_to_path(path, Version::Id3v24)
        .map_err(|error| AppError::Tts(format!("could not write MP3 tags: {error}")))
}

/// The value, unless it is absent or blank. A Source that sends an empty
/// string is saying the same thing as one that sends nothing.
fn present(value: &Option<String>) -> Option<&str> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

/// The attribution as an http(s) URL, or None when it is anything else.
///
/// Parsed rather than prefix-matched, so `mailto:` and a bare author name are
/// both rejected for the same reason instead of by accident.
fn source_url(attribution: &str) -> Option<String> {
    let url = reqwest::Url::parse(attribution).ok()?;
    matches!(url.scheme(), "http" | "https").then(|| url.to_string())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use chrono::Utc;
    use id3::{Tag, TagLike};

    use super::tag_chapter_mp3;
    use crate::db::models::{Document, Section, SourceType};

    /// Not real audio. `id3` writes a tag as a prefix to whatever follows, and
    /// none of these assertions are about the frames after it.
    const FAKE_MP3: &[u8] = b"\xff\xfb\x90\x00 not really audio";

    fn document(license: Option<&str>, attribution: Option<&str>) -> Document {
        Document {
            id: "doc-1".into(),
            title: "Introduction to Philosophy".into(),
            source_type: SourceType::Pressbooks,
            source_metadata: serde_json::json!({}),
            cover_image_path: None,
            license: license.map(str::to_string),
            attribution: attribution.map(str::to_string),
            word_count: 8,
            imported_at: Utc::now(),
            last_opened_at: None,
        }
    }

    fn section() -> Section {
        Section {
            id: "sec-1".into(),
            document_id: "doc-1".into(),
            ordinal: 0,
            title: "Chapter One".into(),
            word_count: 8,
        }
    }

    /// Writes a fake MP3 into `dir`, tags it, and reads the tag back. The
    /// round trip goes through `id3` both ways on purpose: a test that
    /// re-implemented the encoding would only assert my own assumptions.
    fn tagged(dir: &Path, document: &Document) -> Option<Tag> {
        let path = dir.join("chapter.mp3");
        std::fs::write(&path, FAKE_MP3).expect("the fake mp3");
        tag_chapter_mp3(&path, document, &section()).expect("tagging should succeed");
        Tag::read_from_path(&path).ok()
    }

    #[test]
    fn an_exported_chapter_carries_its_book_and_section_titles() {
        let dir = tempfile::tempdir().expect("a temporary export directory");
        let tag = tagged(dir.path(), &document(Some("CC BY 4.0"), None))
            .expect("a tag should have been written");

        assert_eq!(tag.album(), Some("Introduction to Philosophy"));
        assert_eq!(tag.title(), Some("Chapter One"));
    }

    #[test]
    fn the_books_licence_travels_with_the_file() {
        let dir = tempfile::tempdir().expect("a temporary export directory");
        let tag = tagged(dir.path(), &document(Some("CC BY-NC-SA 4.0"), None))
            .expect("a tag should have been written");

        assert_eq!(
            tag.get("TCOP").and_then(|frame| frame.content().text()),
            Some("CC BY-NC-SA 4.0"),
            "the licence must be readable by anything that opens the file"
        );
    }

    #[test]
    fn a_url_attribution_becomes_the_source_webpage_not_the_artist() {
        // OpenStax, LibreTexts and article all store a URL in `attribution`.
        // Writing a URL as the artist puts "https://openstax.org/..." in every
        // music player's artist column.
        let dir = tempfile::tempdir().expect("a temporary export directory");
        let tag = tagged(
            dir.path(),
            &document(
                Some("CC BY 4.0"),
                Some("https://openstax.org/books/biology-2e"),
            ),
        )
        .expect("a tag should have been written");

        assert_eq!(
            tag.get("WOAS").and_then(|frame| frame.content().link()),
            Some("https://openstax.org/books/biology-2e")
        );
        assert_eq!(tag.artist(), None, "a URL is not an artist");
    }

    #[test]
    fn an_author_attribution_becomes_the_artist_not_a_link() {
        // Pressbooks stores an author name there instead. The field is
        // polymorphic by Source, and one mapping is wrong for the other.
        let dir = tempfile::tempdir().expect("a temporary export directory");
        let tag = tagged(
            dir.path(),
            &document(Some("CC BY-NC-SA 4.0"), Some("Craig DeLancey")),
        )
        .expect("a tag should have been written");

        assert_eq!(tag.artist(), Some("Craig DeLancey"));
        assert!(
            tag.get("WOAS").is_none(),
            "an author name is not a source URL"
        );
    }

    #[test]
    fn a_source_that_supplied_neither_leaves_those_frames_out() {
        // A pasted-text import has no licence and no attribution. An empty
        // TCOP claims the file is licensed under nothing in particular, which
        // is worse than saying nothing.
        let dir = tempfile::tempdir().expect("a temporary export directory");
        let tag =
            tagged(dir.path(), &document(None, None)).expect("a tag should have been written");

        assert!(tag.get("TCOP").is_none(), "no licence, no licence frame");
        assert_eq!(tag.artist(), None);
        assert!(tag.get("WOAS").is_none());
        // The titles are always known, so the tag is never empty.
        assert_eq!(tag.album(), Some("Introduction to Philosophy"));
    }
}
