//! Parsing session transcripts into selectable conversation turns.

mod parser;

pub use parser::tail_summary;
pub use parser::{read_transcript, Turn};
