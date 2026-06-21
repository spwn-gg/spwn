//! Parsing session transcripts into selectable conversation turns.

mod parser;

pub use parser::{read_transcript, Turn};
