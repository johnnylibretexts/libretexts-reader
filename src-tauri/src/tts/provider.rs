//! The seam between "some engine speaks" and "which engine speaks".
//! Task 3 adds the `TtsProvider` trait here.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceSummary {
    pub id: String,
    pub name: String,
    pub ready: bool,
}
