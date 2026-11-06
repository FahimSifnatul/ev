const MAX_VALUE: usize = 40;
const HEADER: [&str; 4] = ["NAME", "VALUE", "SOURCE", "DESCRIPTION"];

/// One table line: name, value, source, description.
pub type Row = (String, String, String, String);

/// Aligned table of rows; empty when there are no rows.
pub fn table(rows: &[Row]) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let cells: Vec<[String; 4]> = rows
        .iter()
        .map(|(name, value, source, desc)| {
            [name.clone(), shorten(value), source.clone(), desc.clone()]
        })
        .collect();
    let width = |col: usize| {
        cells
            .iter()
            .map(|row| row[col].chars().count())
            .chain([HEADER[col].len()])
            .max()
            .unwrap_or(0)
    };
    let widths = [width(0), width(1), width(2)];

    let line = |row: [&str; 4]| {
        let name = pad(row[0], widths[0]);
        let value = pad(row[1], widths[1]);
        let source = pad(row[2], widths[2]);
        format!("{name}  {value}  {source}  {}", row[3]).trim_end().to_string()
    };
    let mut out = format!("{}\n", line(HEADER));
    for row in &cells {
        out.push_str(&line([&row[0], &row[1], &row[2], &row[3]]));
        out.push('\n');
    }
    out
}

fn pad(text: &str, width: usize) -> String {
    format!("{text:<width$}")
}

fn shorten(value: &str) -> String {
    if value.chars().count() <= MAX_VALUE {
        return value.to_string();
    }
    let head: String = value.chars().take(MAX_VALUE - 1).collect();
    format!("{head}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, value: &str, desc: &str) -> Row {
        src_row(name, value, "ev", desc)
    }

    fn src_row(name: &str, value: &str, source: &str, desc: &str) -> Row {
        (name.to_string(), value.to_string(), source.to_string(), desc.to_string())
    }

    #[test]
    fn has_header_row() {
        let out = table(&[row("A", "1", "one")]);
        assert!(out.starts_with("NAME  VALUE  SOURCE  DESCRIPTION\n"));
    }

    #[test]
    fn columns_align_to_longest_cell() {
        let out = table(&[
            src_row("A", "1", "ev", "one"),
            src_row("LONGER", "value", "~/.zshrc", "two"),
        ]);
        let lines: Vec<&str> = out.lines().skip(1).collect();
        assert_eq!(
            lines,
            ["A       1      ev        one", "LONGER  value  ~/.zshrc  two"]
        );
    }

    #[test]
    fn source_column_shows_each_source() {
        let out = table(&[src_row("A", "1", "env", ""), src_row("B", "2", "/etc/profile", "")]);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "NAME  VALUE  SOURCE        DESCRIPTION");
        assert_eq!(lines[1], "A     1      env");
        assert_eq!(lines[2], "B     2      /etc/profile");
    }

    #[test]
    fn empty_description_leaves_no_trailing_spaces() {
        let out = table(&[row("A", "1", "")]);
        assert_eq!(out.lines().nth(1), Some("A     1      ev"));
    }

    #[test]
    fn value_of_exactly_40_chars_stays_whole() {
        let value = "x".repeat(40);
        assert!(table(&[row("A", &value, "")]).contains(&value));
    }

    #[test]
    fn longer_value_is_cut_with_ellipsis() {
        let out = table(&[row("A", &"x".repeat(41), "d")]);
        let cut = format!("{}…", "x".repeat(39));
        assert!(out.contains(&format!("A     {cut}  ev      d")));
    }

    #[test]
    fn multibyte_value_is_cut_on_chars_and_stays_aligned() {
        let out = table(&[row("A", &"é".repeat(41), "d"), row("B", "1", "e")]);
        let cut = format!("{}…", "é".repeat(39));
        let lines: Vec<&str> = out.lines().skip(1).collect();
        assert_eq!(lines[0], format!("A     {cut}  ev      d"));
        assert_eq!(lines[1], format!("B     {:<40}  ev      e", "1"));
    }

    #[test]
    fn long_description_is_never_cut() {
        let desc = "d".repeat(100);
        assert!(table(&[row("A", "1", &desc)]).contains(&desc));
    }

    #[test]
    fn no_rows_no_output() {
        assert_eq!(table(&[]), "");
    }
}
