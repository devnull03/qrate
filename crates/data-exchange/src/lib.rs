//! Everything that moves a collection across qrate's boundary: fetching a Google Sheet in,
//! writing CSV / JSON-LD / CSL-JSON / a ZIP archive out, and creating a Sheet in the user's Drive.
//!
//! Nothing here touches gpui. Callers read the grid and pick a path, then hand this crate plain
//! values — which is what keeps the parsers and writers testable without a window.

pub mod export;
pub mod google;
pub mod preview;
pub mod sheet;

pub use preview::{PreviewNote, SpreadsheetError, SpreadsheetPreview};
pub use sheet::{CellNote, SheetData, SheetSyncError, a1_to_index, fetch_sheet};
