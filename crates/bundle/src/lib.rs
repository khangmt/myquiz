//! The quiz zip: one `quiz.json` plus images under `media/`
//! (FR-QZ-15 to FR-QZ-17, design section 8).
//!
//! ```text
//! myquiz-<title-slug>-<yyyymmdd>.zip
//! ├── quiz.json        required (or quiz.yaml / quiz.yml); format name + version
//! ├── media/
//! │   └── q3.png       names: [a-z0-9_-]{1,64}.(jpg|jpeg|png|webp)
//! └── README.txt       written on export, ignored on import
//! ```
//!
//! The same quiz can also travel as YAML: `quiz.yaml` inside the zip, or,
//! when the quiz has no images, a plain `.yaml` file ([`read_yaml`],
//! [`to_yaml`]). Both formats carry exactly the same fields.
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

pub use read::{read_bundle, read_yaml, Bundle, Limits, Problem};
pub use write::{
    file_name, file_name_ext, template, to_yaml, write_bundle, write_bundle_as, Manifest, README,
    YAML_HEADER,
};
