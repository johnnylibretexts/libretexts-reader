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

/// Write the chapter's identity, licence and credit into the exported file.
///
/// Dispatches on the extension because the two providers no longer share a
/// container: Supertonic encodes AAC/M4A locally (ADR-0004) and Fish returns
/// MP3 from its API. ID3 frames do not exist in an MP4 container, so a single
/// tagger cannot serve both -- and writing an ID3 tag onto an M4A would
/// corrupt it rather than fail, which is why this matches explicitly and
/// errors on anything it does not recognise instead of guessing.
pub(crate) fn tag_chapter_export(
    path: &Path,
    document: &Document,
    section: &Section,
    target_lang: Option<&str>,
) -> AppResult<()> {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("mp3") => tag_chapter_mp3(path, document, section, target_lang),
        Some("m4a") | Some("mp4") | Some("m4b") => {
            tag_chapter_mp4(path, document, section, target_lang)
        }
        other => Err(AppError::Tts(format!(
            "cannot tag an export with extension {}",
            other.unwrap_or("(none)")
        ))),
    }
}

/// The MP4/M4A half: iTunes-style atoms rather than ID3 frames.
///
/// The frame choices mirror `tag_chapter_mp3` deliberately, so a reader who
/// exports the same chapter through either provider gets the same facts in
/// whatever their player calls those fields. MP4 has no dedicated copyright or
/// source-URL atom pair matching TCOP/WOAS, so both land where a player will
/// actually surface them.
fn tag_chapter_mp4(
    path: &Path,
    document: &Document,
    section: &Section,
    target_lang: Option<&str>,
) -> AppResult<()> {
    let mut tag = mp4ameta::Tag::read_from_path(path)
        .map_err(|error| AppError::Tts(format!("could not read M4A tags: {error}")))?;

    tag.set_album(&document.title);
    tag.set_title(&section.title);
    if let Some(language) = target_lang {
        tag.set_data(
            mp4ameta::FreeformIdent::new("com.apple.iTunes", "LANGUAGE"),
            mp4ameta::Data::Utf8(language.to_string()),
        );
    }

    if let Some(license) = present(&document.license) {
        tag.set_copyright(license);
    }

    if let Some(attribution) = present(&document.attribution) {
        // Same polymorphism as the MP3 path: a URL for OpenStax, LibreTexts
        // and article; an author name for Pressbooks. A URL in the artist
        // field fills a player's artist column with a link, so it goes to the
        // comment instead, where a long string is expected.
        match source_url(attribution) {
            Some(url) => tag.set_comment(url),
            None => tag.set_artist(attribution),
        }
    }

    tag.write_to_path(path)
        .map_err(|error| AppError::Tts(format!("could not write M4A tags: {error}")))
}

fn tag_chapter_mp3(
    path: &Path,
    document: &Document,
    section: &Section,
    target_lang: Option<&str>,
) -> AppResult<()> {
    let mut tag = Tag::new();

    // Always known, so the tag is never empty even for a Source that supplied
    // no licence at all.
    tag.set_album(&document.title);
    tag.set_title(&section.title);
    if let Some(language) = target_lang {
        tag.add_frame(Frame::text("TLAN", language));
    }

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
            progress: 0.0,
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
        tag_chapter_mp3(&path, document, &section(), None).expect("tagging should succeed");
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
    fn a_translated_mp3_carries_its_target_language() {
        let dir = tempfile::tempdir().expect("a temporary export directory");
        let path = dir.path().join("chapter.mp3");
        std::fs::write(&path, FAKE_MP3).expect("the fake mp3");
        tag_chapter_mp3(&path, &document(None, None), &section(), Some("es"))
            .expect("tagging should succeed");

        let tag = Tag::read_from_path(&path).expect("a tag should have been written");
        assert_eq!(
            tag.get("TLAN").and_then(|frame| frame.content().text()),
            Some("es")
        );
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

    /// Round-trips through the real AudioToolbox encoder rather than a fake
    /// file: mp4ameta parses the container it is handed, so a handmade stub
    /// would test the stub. macOS-only for the same reason the encoder is.
    #[cfg(target_os = "macos")]
    #[test]
    fn writes_licence_and_credit_into_an_m4a() {
        use crate::tts::supertonic::audio::{encode_f32_to_m4a, SUPERTONIC_SAMPLE_RATE};

        let samples: Vec<f32> = (0..44_100)
            .map(|i| (i as f32 / 100.0).sin() * 0.5)
            .collect();
        let audio = encode_f32_to_m4a(&samples, SUPERTONIC_SAMPLE_RATE).unwrap();
        let dir = std::env::temp_dir().join(format!("tags-m4a-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("chapter.m4a");
        std::fs::write(&path, &audio).unwrap();

        super::tag_chapter_export(
            &path,
            &document(Some("CC BY 4.0"), Some("Jane Author")),
            &section(),
            Some("es"),
        )
        .expect("tagging an m4a should succeed");

        let tag = mp4ameta::Tag::read_from_path(&path).unwrap();
        assert_eq!(tag.album(), Some("Introduction to Philosophy"));
        assert_eq!(tag.title(), Some("Chapter One"));
        assert_eq!(tag.copyright(), Some("CC BY 4.0"));
        // A plain name is an artist; a URL would go to the comment instead.
        assert_eq!(tag.artist(), Some("Jane Author"));
        let language = mp4ameta::FreeformIdent::new("com.apple.iTunes", "LANGUAGE");
        assert_eq!(tag.strings_of(&language).next(), Some("es"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn refuses_a_container_it_cannot_tag() {
        // Silently skipping would drop the licence and credit from any future
        // format, which is the failure #97 existed to prevent.
        let dir = std::env::temp_dir().join(format!("tags-unknown-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("chapter.ogg");
        std::fs::write(&path, b"not audio").unwrap();

        let error =
            super::tag_chapter_export(&path, &document(None, None), &section(), None).unwrap_err();

        assert!(format!("{error}").contains("ogg"), "unexpected: {error}");
        std::fs::remove_dir_all(&dir).ok();
    }
}
