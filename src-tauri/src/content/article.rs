use std::io::Cursor;
use std::time::Duration;

use chrono::Utc;
use readability::extractor;
use reqwest::Url;
use serde_json::json;

use crate::content::document::{DocumentBuilder, SectionBuilder};
use crate::content::split_paragraphs;
use crate::db::models::SourceType;
use crate::error::{AppError, AppResult};

const MAX_ARTICLE_BYTES: u64 = 5 * 1024 * 1024;

pub async fn import_from_url(url: &str) -> AppResult<DocumentBuilder> {
    let parsed_url = parse_article_url(url)?;
    let html = fetch_article_html(parsed_url.as_str()).await?;
    let product = extractor::extract(&mut Cursor::new(html), &parsed_url)?;
    let paragraphs = split_paragraphs(&product.text);

    if paragraphs.is_empty() {
        return Err(AppError::InvalidInput(
            "no readable article content found".into(),
        ));
    }

    let title = if product.title.trim().is_empty() {
        parsed_url
            .host_str()
            .map_or_else(|| "Article".to_string(), ToOwned::to_owned)
    } else {
        product.title.trim().to_string()
    };

    Ok(DocumentBuilder {
        title,
        source_type: SourceType::Url,
        source_metadata: json!({
            "url": parsed_url.as_str(),
            "fetched_at": Utc::now().to_rfc3339()
        }),
        cover_image_path: None,
        license: Some("Unknown - see source URL".to_string()),
        attribution: Some(parsed_url.as_str().to_string()),
        sections: vec![SectionBuilder {
            title: "Content".to_string(),
            paragraphs,
        }],
    })
}

fn parse_article_url(url: &str) -> AppResult<Url> {
    let parsed = Url::parse(url.trim())
        .map_err(|_| AppError::InvalidInput("URL must be a valid http or https URL".into()))?;

    match parsed.scheme() {
        "http" | "https" => Ok(parsed),
        _ => Err(AppError::InvalidInput(
            "URL must start with http:// or https://".into(),
        )),
    }
}

async fn fetch_article_html(url: &str) -> AppResult<Vec<u8>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()?;
    let response = client.get(url).send().await?;
    let status = response.status();

    if !status.is_success() {
        return Err(AppError::InvalidInput(format!(
            "article fetch failed with HTTP {status}"
        )));
    }

    if response
        .content_length()
        .is_some_and(|length| length > MAX_ARTICLE_BYTES)
    {
        return Err(AppError::InvalidInput(
            "article is larger than the 5 MB import limit".into(),
        ));
    }

    let bytes = response.bytes().await?;
    if bytes.len() as u64 > MAX_ARTICLE_BYTES {
        return Err(AppError::InvalidInput(
            "article is larger than the 5 MB import limit".into(),
        ));
    }

    Ok(bytes.to_vec())
}
