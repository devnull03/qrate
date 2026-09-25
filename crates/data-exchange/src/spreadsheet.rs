//! Local CSV, TSV, Excel, and OpenDocument grids shared by project creation and row import.

use std::path::Path;

use calamine::{Data, Reader, open_workbook_auto};

use crate::SpreadsheetError;

/// Read the first workbook tab, or the whole delimited file, as headers and text cells.
pub fn read_grid(path: &str) -> Result<(Vec<String>, Vec<Vec<String>>), SpreadsheetError> {
    let path = Path::new(path);
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();

    match extension.as_str() {
        "csv" | "tsv" => {
            let mut reader = csv::ReaderBuilder::new()
                .has_headers(true)
                .flexible(true)
                .delimiter(if extension == "tsv" { b'\t' } else { b',' })
                .from_path(path)
                .map_err(|error| SpreadsheetError::Io(error.to_string()))?;
            let headers = reader
                .headers()
                .map_err(|error| SpreadsheetError::Io(error.to_string()))?
                .iter()
                .map(str::to_string)
                .collect();
            let mut rows = Vec::new();
            for record in reader.records() {
                let record = record.map_err(|error| SpreadsheetError::Io(error.to_string()))?;
                rows.push(record.iter().map(str::to_string).collect());
            }
            Ok((headers, rows))
        }
        "xlsx" | "xlsm" | "xlsb" | "xls" | "ods" => {
            let mut workbook = open_workbook_auto(path)
                .map_err(|error| SpreadsheetError::Io(error.to_string()))?;
            let name = workbook
                .sheet_names()
                .first()
                .cloned()
                .ok_or(SpreadsheetError::Empty)?;
            let range = workbook
                .worksheet_range(&name)
                .map_err(|error| SpreadsheetError::Io(error.to_string()))?;
            let mut grid = range.rows().map(|row| row.iter().map(cell_text).collect());
            let headers = grid.next().unwrap_or_default();
            Ok((headers, grid.collect()))
        }
        _ => Err(SpreadsheetError::UnsupportedFormat),
    }
}

pub(crate) fn cell_text(cell: &Data) -> String {
    match cell {
        Data::Empty => String::new(),
        Data::DateTime(value) => match value.as_datetime() {
            Some(date) if date.time() == Default::default() => date.format("%Y-%m-%d").to_string(),
            Some(date) => date.format("%Y-%m-%d %H:%M:%S").to_string(),
            None => value.as_f64().to_string(),
        },
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_excel_headers_and_text_rows() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("import.xlsx");
        let mut workbook = rust_xlsxwriter::Workbook::new();
        let sheet = workbook.add_worksheet();
        sheet.write_string(0, 0, "Digital ID").unwrap();
        sheet.write_string(0, 1, "Title").unwrap();
        sheet.write_string(1, 0, "0012").unwrap();
        sheet.write_string(1, 1, "Object").unwrap();
        workbook.save(&path).unwrap();

        let (headers, rows) = super::read_grid(path.to_str().unwrap()).unwrap();
        assert_eq!(headers, ["Digital ID", "Title"]);
        assert_eq!(rows, [["0012", "Object"]]);
    }
}
