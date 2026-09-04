//! セッション保存・復元
//! 前回の入力内容を <schematic>.bus_gen.json に保存する

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SavedRow {
    pub reference:  String,
    pub pin_prefix: String,
    pub prefix:     String,
    pub start:      i32,
    pub end:        i32,
    pub wire_len:   f64,
}

pub fn session_path(sch_path: &Path) -> PathBuf {
    sch_path.with_extension("bus_gen.json")
}

pub fn load(sch_path: &Path) -> Vec<SavedRow> {
    let path = session_path(sch_path);
    let Ok(data) = std::fs::read_to_string(&path) else { return vec![] };
    serde_json::from_str(&data).unwrap_or_default()
}

pub fn save(sch_path: &Path, rows: &[SavedRow]) {
    let path = session_path(sch_path);
    if let Ok(json) = serde_json::to_string_pretty(rows) {
        let _ = std::fs::write(path, json);
    }
}
