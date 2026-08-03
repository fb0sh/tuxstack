//! Output helpers for the CLI: tables, plain text, and JSON.

use std::io::Write;

use tuxstack_docker_core::format::bytes;

/// Serialize anything to a JSON value for `--json` output.
pub fn to_json<T: serde::Serialize>(value: &T) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(value)
}

/// Print JSON to stdout.
pub fn print_json<T: serde::Serialize>(value: &T) -> Result<(), serde_json::Error> {
    println!("{}", to_json(value)?);
    Ok(())
}

/// A simple columnar table.
pub struct Table {
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
}

impl Table {
    pub fn new(headers: Vec<&str>) -> Self {
        Self {
            headers: headers.into_iter().map(|h| h.to_string()).collect(),
            rows: Vec::new(),
        }
    }

    pub fn row(&mut self, cells: Vec<String>) {
        self.rows.push(cells);
    }

    /// Render the table to a writer, aligning columns by padding.
    pub fn render(&self, w: &mut impl Write) -> std::io::Result<()> {
        let mut widths: Vec<usize> = self.headers.iter().map(|h| h.chars().count()).collect();
        for row in &self.rows {
            for (i, cell) in row.iter().enumerate() {
                if let Some(width) = widths.get_mut(i) {
                    *width = (*width).max(cell.chars().count());
                }
            }
        }

        fn render_row<W: Write>(
            w: &mut W,
            cells: &[String],
            widths: &[usize],
        ) -> std::io::Result<()> {
            let line = cells
                .iter()
                .enumerate()
                .map(|(i, cell)| {
                    let width = widths.get(i).copied().unwrap_or(0);
                    if i + 1 == cells.len() {
                        cell.clone()
                    } else {
                        format!("{cell:<width$}   ")
                    }
                })
                .collect::<String>();
            writeln!(w, "{}", line.trim_end())
        }

        render_row(w, &self.headers, &widths)?;
        writeln!(
            w,
            "{}",
            "-".repeat(widths.iter().sum::<usize>() + widths.len() * 3)
        )?;
        for row in &self.rows {
            render_row(w, row, &widths)?;
        }
        Ok(())
    }
}

/// Human readable byte size.
pub fn size_cell(byte_count: u64) -> String {
    bytes(byte_count)
}
