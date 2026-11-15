//! Reading and writing comma-separated tables (RFC 4180), for the files a
//! spreadsheet opens and saves.
//!
//! Read: UTF-8, with or without a byte-order mark; lines end in LF or CRLF; a
//! cell in double quotes may hold commas, line breaks and doubled quotes; a
//! row whose cells are all empty is skipped. Written: UTF-8, LF line ends, a
//! cell quoted only when it needs to be.

/// One row of a table and the line of the file it started on (from 1).
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub line: usize,
    pub cells: Vec<String>,
}

/// Split CSV text into rows.
pub fn parse(text: &str) -> Result<Vec<Row>, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut rows = Vec::new();
    let mut cells: Vec<String> = Vec::new();
    let mut cell = String::new();
    let mut line = 1;
    let mut row_line = 1;
    let mut quoted = false;
    // Whether the current cell was opened with a quote and has been closed.
    let mut closed = false;
    let mut chars = text.chars().peekable();
    let end_row = |cells: &mut Vec<String>, cell: &mut String, rows: &mut Vec<Row>, at: usize| {
        cells.push(std::mem::take(cell));
        let taken = std::mem::take(cells);
        if taken.iter().any(|value| !value.is_empty()) {
            rows.push(Row {
                line: at,
                cells: taken,
            });
        }
    };
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    cell.push('"');
                }
                '"' => {
                    quoted = false;
                    closed = true;
                }
                '\n' => {
                    line += 1;
                    cell.push('\n');
                }
                other => cell.push(other),
            }
            continue;
        }
        match c {
            '"' if cell.is_empty() && !closed => quoted = true,
            '"' => return Err(format!("line {line}: a quote inside an unquoted cell")),
            ',' => {
                cells.push(std::mem::take(&mut cell));
                closed = false;
            }
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' => {
                end_row(&mut cells, &mut cell, &mut rows, row_line);
                closed = false;
                line += 1;
                row_line = line;
            }
            _ if closed => {
                return Err(format!("line {line}: text after a closing quote"));
            }
            other => cell.push(other),
        }
    }
    if quoted {
        return Err(format!("line {row_line}: a quoted cell is never closed"));
    }
    if !cell.is_empty() || !cells.is_empty() {
        end_row(&mut cells, &mut cell, &mut rows, row_line);
    }
    Ok(rows)
}

/// One cell, quoted if it holds a comma, a quote, a line break, or leading or
/// trailing space.
fn write_cell(cell: &str, out: &mut String) {
    let needs_quotes =
        cell.contains([',', '"', '\n', '\r']) || cell.starts_with(' ') || cell.ends_with(' ');
    if needs_quotes {
        out.push('"');
        out.push_str(&cell.replace('"', "\"\""));
        out.push('"');
    } else {
        out.push_str(cell);
    }
}

/// Rows as CSV text.
pub fn write<R, C>(rows: impl IntoIterator<Item = R>) -> String
where
    R: IntoIterator<Item = C>,
    C: AsRef<str>,
{
    let mut out = String::new();
    for row in rows {
        for (index, cell) in row.into_iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            write_cell(cell.as_ref(), &mut out);
        }
        out.push('\n');
    }
    out
}

/// A table with a header row, read against the columns it may have.
#[derive(Debug)]
pub struct Table {
    /// File name, for messages.
    pub name: String,
    /// For each known column, its position in the file, if present.
    index: Vec<Option<usize>>,
    columns: &'static [&'static str],
    pub rows: Vec<Row>,
}

impl Table {
    /// Parse `text`. The first row names the columns: every name must be one
    /// of `columns`, none may repeat, and every one of `required` must be
    /// there. Column order in the file is free.
    pub fn read(
        name: &str,
        text: &str,
        columns: &'static [&'static str],
        required: &[&str],
    ) -> Result<Table, Vec<String>> {
        let mut rows = parse(text).map_err(|e| vec![format!("{name}: {e}")])?;
        if rows.is_empty() {
            return Err(vec![format!("{name}: no header row")]);
        }
        let header = rows.remove(0);
        let mut errors = Vec::new();
        let mut index = vec![None; columns.len()];
        for (position, cell) in header.cells.iter().enumerate() {
            let cell = cell.trim();
            if cell.is_empty() {
                continue;
            }
            match columns.iter().position(|known| *known == cell) {
                None => errors.push(format!(
                    "{name}: row {}: unknown column {cell:?}; the columns are {}",
                    header.line,
                    columns.join(", ")
                )),
                Some(column) if index[column].is_some() => errors.push(format!(
                    "{name}: row {}: column {cell:?} appears twice",
                    header.line
                )),
                Some(column) => index[column] = Some(position),
            }
        }
        for column in required {
            let position = columns.iter().position(|known| known == column);
            if position.and_then(|p| index[p]).is_none() {
                errors.push(format!("{name}: no column {column:?}"));
            }
        }
        for row in &rows {
            if row.cells.len() > header.cells.len()
                && row.cells[header.cells.len()..]
                    .iter()
                    .any(|cell| !cell.trim().is_empty())
            {
                errors.push(format!(
                    "{name}: row {}: more cells than the header has columns",
                    row.line
                ));
            }
        }
        if errors.is_empty() {
            Ok(Table {
                name: name.to_string(),
                index,
                columns,
                rows,
            })
        } else {
            Err(errors)
        }
    }

    /// The trimmed text of a column in a row; empty when the column or the
    /// cell is absent.
    pub fn cell<'a>(&self, row: &'a Row, column: &str) -> &'a str {
        self.columns
            .iter()
            .position(|known| *known == column)
            .and_then(|position| self.index[position])
            .and_then(|position| row.cells.get(position))
            .map_or("", |cell| cell.trim())
    }

    /// The text of a column in a row as written, spaces included.
    pub fn raw<'a>(&self, row: &'a Row, column: &str) -> &'a str {
        self.columns
            .iter()
            .position(|known| *known == column)
            .and_then(|position| self.index[position])
            .and_then(|position| row.cells.get(position))
            .map_or("", String::as_str)
    }

    /// `<file>: row <n>`, to start a message about a row.
    pub fn at(&self, row: &Row) -> String {
        format!("{}: row {}", self.name, row.line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cells(text: &str) -> Vec<Vec<String>> {
        parse(text)
            .unwrap()
            .into_iter()
            .map(|row| row.cells)
            .collect()
    }

    #[test]
    fn plain_rows_with_either_line_ending() {
        assert_eq!(cells("a,b\n1,2\n"), [["a", "b"], ["1", "2"]]);
        assert_eq!(cells("a,b\r\n1,2\r\n"), [["a", "b"], ["1", "2"]]);
        assert_eq!(cells("a,b\n1,2"), [["a", "b"], ["1", "2"]]);
        assert_eq!(cells("\u{feff}a,b\n"), [["a", "b"]]);
    }

    #[test]
    fn quoted_cells() {
        assert_eq!(
            cells("\"a,b\",\"say \"\"hi\"\"\",\"two\nlines\",\n"),
            [["a,b", "say \"hi\"", "two\nlines", ""]]
        );
        assert_eq!(cells("\"\",x\n"), [["", "x"]]);
    }

    #[test]
    fn empty_rows_are_skipped_and_lines_are_counted() {
        let rows = parse("h1,h2\n\n,,\n\"x\ny\",1\nz,2\n").unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1].line, 4);
        assert_eq!(rows[2].line, 6);
    }

    #[test]
    fn malformed_text_names_the_line() {
        assert_eq!(
            parse("a\n\"b").unwrap_err(),
            "line 2: a quoted cell is never closed"
        );
        assert_eq!(
            parse("a\nb\"c\n").unwrap_err(),
            "line 2: a quote inside an unquoted cell"
        );
        assert_eq!(
            parse("\"a\"b\n").unwrap_err(),
            "line 1: text after a closing quote"
        );
    }

    #[test]
    fn writing_quotes_only_what_needs_it_and_reads_back() {
        let rows = vec![
            vec!["plain", "a,b", "say \"hi\"", "two\nlines", " padded ", ""],
            vec!["µs", "x", "y", "z", "w", "v"],
        ];
        let text = write(rows.clone());
        assert_eq!(
            text,
            "plain,\"a,b\",\"say \"\"hi\"\"\",\"two\nlines\",\" padded \",\nµs,x,y,z,w,v\n"
        );
        assert_eq!(cells(&text), rows);
    }

    #[test]
    fn a_table_refuses_columns_it_does_not_know() {
        const COLUMNS: &[&str] = &["name", "start", "stop"];
        let errors = Table::read(
            "t.csv",
            "name,begin,stop\nx,1,2\n",
            COLUMNS,
            &["name", "start"],
        )
        .unwrap_err();
        assert_eq!(errors.len(), 2, "{errors:?}");
        assert!(errors[0].contains("row 1: unknown column \"begin\""));
        assert_eq!(errors[1], "t.csv: no column \"start\"");
        let table = Table::read("t.csv", "stop, name\n2,x\n", COLUMNS, &["name"]).unwrap();
        assert_eq!(table.cell(&table.rows[0], "name"), "x");
        assert_eq!(table.cell(&table.rows[0], "start"), "");
        assert_eq!(table.at(&table.rows[0]), "t.csv: row 2");
    }
}
