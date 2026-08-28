//! Output formats.

pub mod fdx;
pub mod html;
pub mod text;

/// The formats a screenplay can be exported as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Fixed-width text at industry margins.
    Text,
    /// Printable HTML (print to PDF for a submission-ready script).
    Html,
    /// Final Draft XML.
    Fdx,
}

impl Format {
    /// Which format a name the writer chose in the Export dialog asks for.
    ///
    /// A `&Path` rather than a `&str`: a file name is not always valid UTF-8,
    /// and a screenplay exported to one should still come out in the format its
    /// extension asked for rather than silently falling back.
    pub fn from_path(path: &std::path::Path) -> Option<Format> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "txt" => Some(Format::Text),
            "html" | "htm" => Some(Format::Html),
            "fdx" => Some(Format::Fdx),
            _ => None,
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Format::Text => "txt",
            Format::Html => "html",
            Format::Fdx => "fdx",
        }
    }
}
