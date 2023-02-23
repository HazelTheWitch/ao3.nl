#![feature(iter_intersperse)]

use serde::{Deserialize, Serialize};

pub mod ao3;

#[derive(Deserialize, Serialize)]
pub struct EmbedRequest {
    pub id: u64,
    pub author: String,
    pub words: u64,
    pub chapters: u16,
    pub total_chapters: String,
    pub date: String,
}