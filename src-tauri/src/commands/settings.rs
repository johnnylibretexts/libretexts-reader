use std::collections::HashMap;

use serde_json::Value;
use tauri::State;

use crate::db::connection::DbPool;
use crate::db::settings;
use crate::error::AppResult;

#[tauri::command]
pub async fn get_setting(state: State<'_, DbPool>, key: String) -> AppResult<Option<Value>> {
    let conn = state.get()?;
    settings::get_setting(&conn, &key)
}

#[tauri::command]
pub async fn set_setting(state: State<'_, DbPool>, key: String, value: Value) -> AppResult<()> {
    let conn = state.get()?;
    settings::set_setting(&conn, &key, &value)
}

#[tauri::command]
pub async fn get_all_settings(state: State<'_, DbPool>) -> AppResult<HashMap<String, Value>> {
    let conn = state.get()?;
    settings::get_all_settings(&conn)
}
