//! Imports and desktop Google Sheets integration.
//!
//! Nothing here touches gpui. Callers read the grid and pick a path, then hand this crate plain
//! values, keeping the parsers testable without a window.

pub mod google;
pub mod preview;
pub mod sheet;
pub mod spreadsheet;

pub use preview::{PreviewNote, SpreadsheetError, SpreadsheetPreview};
pub use sheet::{CellNote, SheetData, SheetSyncError, a1_to_index, fetch_sheet};
