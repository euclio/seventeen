use std::fmt::Write;
use std::io;

use termion::{clear, cursor, style};

use super::line_cache::LineCache;

#[derive(Debug)]
pub struct Window {
    pub line_cache: LineCache,
    pub rows: u16,
    pub cols: u16,
}

impl Window {
    pub fn render<W: io::Write>(&self, writer: &mut W) -> io::Result<()> {
        let mut sequences = String::new();

        let mut cursors = self.line_cache.iter_cursors().collect::<Vec<_>>();

        // Sort cursors so that we can iterate through them top to bottom, left to right
        cursors.sort_by_key(|cursor| (cursor.y, cursor.x));

        for (i, line) in self
            .line_cache
            .iter_lines()
            .enumerate()
            .take(self.rows as usize)
        {
            let line_no = (i + 1) as u16;

            // There might be a newline at the end of the current line, but we're already iterating
            // linewise.
            let mut line = String::from(line.trim_right_matches('\n'));

            // Add any cursors or styles to the current line. We have to add them in reverse order
            // so we don't invalidate the indices.
            for cursor in cursors.iter().rev() {
                if cursor.y == line_no {
                    insert_cursor(&mut line, u64::from(cursor.x - 1));
                }
            }

            write!(
                sequences,
                "{}{}{}",
                cursor::Goto(1, line_no),
                clear::CurrentLine,
                line
            ).unwrap();
        }

        // If there are more rows in the window than lines, then fill them out with tildes.
        for i in (self.line_cache.len() as u16)..=self.rows {
            write!(
                sequences,
                "{}{}~",
                cursor::Goto(1, i as u16),
                clear::CurrentLine
            ).unwrap();
        }

        write!(writer, "{}", sequences)?;
        writer.flush()?;

        Ok(())
    }
}

/// Draws a "cursor" at the given byte index in the line.
///
/// Since a terminal can only support a single cursor at a time, we simulate multiple cursors by
/// drawing a reverse highlight wherever we need a cursor.
fn insert_cursor(line: &mut String, index: u64) {
    // Event if the line is empty, we still need to have a character on the line to highlight.
    if line.is_empty() {
        *line = String::from(" ");
    }

    let index = index as usize;
    let c = line.chars().nth(index).unwrap();

    // FIXME: The range here might not work for multibyte characters. Write a test.
    line.replace_range(
        index..index + 1,
        &format!("{}{}{}", style::Invert, c, style::NoInvert),
    );
}

#[cfg(test)]
mod tests {
    use termion::style;

    #[test]
    fn insert_cursor() {
        let mut line = String::from("");
        super::insert_cursor(&mut line, 0);
        assert_eq!(line, format!("{} {}", style::Invert, style::NoInvert));

        let mut line = String::from("test");
        super::insert_cursor(&mut line, 0);
        assert_eq!(line, format!("{}t{}est", style::Invert, style::NoInvert));

        let mut line = String::from("Hello, world!");
        super::insert_cursor(&mut line, 7);
        assert_eq!(
            line,
            format!("Hello, {}w{}orld!", style::Invert, style::NoInvert),
        );
    }
}
