//! The quiz zip: one `quiz.json` plus images under `media/`
//! (FR-QZ-15 to FR-QZ-17, design section 8).
//!
//! ```text
//! myquiz-<title-slug>-<yyyymmdd>.zip
//! ├── quiz.json        required; format name + version
//! ├── media/
//! │   └── q3.png       names: [a-z0-9_-]{1,64}.(jpg|jpeg|png|webp)
//! └── README.txt       written on export, ignored on import
//! ```
//!
//! [`read_bundle`] is the import side and treats the file as hostile: the
//! structure is checked before any content is read, every entry is read
//! through a byte counter, and every problem is collected into one list
//! (SEC-31, SEC-32, SEC-33). Images come back as raw bytes; the caller must
//! still pass them through the media pipeline (decode, re-encode, strip
//! metadata) before storing them. [`write_bundle`] is the export side.

pub mod format;
mod read;
mod write;

pub use read::{read_bundle, Bundle, Limits, Problem};
pub use write::{file_name, template, write_bundle, README};
