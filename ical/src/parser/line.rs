use std::io::{self, BufRead, Write};

const MAX_LINE_LEN: usize = 75;

/// Reads lines according to RFC 5545.
///
/// Logical lines are split into potentially multiple physical lines, each at most 75 bytes long
/// according to RFC 5545. For that reason, this reader merges these physical lines together to
/// logical lines.
pub struct LineReader<R: BufRead> {
    reader: R,
    last: Option<String>,
}

impl<R: BufRead> LineReader<R> {
    /// Creates a new [`LineReader`] from given [`BufRead`] implementation.
    pub fn new(reader: R) -> Self {
        Self { reader, last: None }
    }
}

impl<R: BufRead> Iterator for LineReader<R> {
    type Item = String;

    fn next(&mut self) -> Option<Self::Item> {
        let mut next_line = String::new();

        if let Some(last) = self.last.take() {
            next_line = last;
        } else {
            for line in self.reader.by_ref().lines() {
                let line = line.ok()?;
                if !line.is_empty() {
                    next_line = line;
                    break;
                }
            }
        }

        for line in self.reader.by_ref().lines() {
            let line = line.ok()?;
            if line.is_empty() {
                continue;
            }

            if line.starts_with(' ') || line.starts_with('\t') {
                next_line.push_str(&line[1..]);
            } else {
                self.last = Some(line);
                break;
            }
        }

        if next_line.is_empty() {
            None
        } else {
            Some(next_line)
        }
    }
}

/// Writes lines according to RFC 5545.
///
/// Conversely to [`LineReader`], this writer splits the given logical lines into potentially
/// multiple physical lines, each at most 75 bytes long as required by RFC 5545. These physical
/// lines are written into the given [`Write`] implementation.
pub struct LineWriter<W: Write> {
    writer: W,
}

impl<W: Write> LineWriter<W> {
    /// Creates a new [`LineWriter`] with given [`Write`] implementation.
    pub fn new(writer: W) -> Self {
        Self { writer }
    }

    /// Writes the given line into the underlying [`Write`] implementation.
    pub fn write_line<S: AsRef<str>>(&mut self, line: S) -> io::Result<()> {
        let mut first = true;
        let mut line = line.as_ref();
        while !line.is_empty() {
            let mut left = MAX_LINE_LEN;
            if !first {
                self.writer.write_all(b" ")?;
                left -= 1;
            }

            let total = left;
            for (pos, c) in line.char_indices() {
                if left < c.len_utf8() {
                    break;
                }
                self.writer
                    .write_all(line[pos..pos + c.len_utf8()].as_bytes())?;
                left -= c.len_utf8();
            }

            self.writer.write_all(b"\r\n")?;
            line = &line[(total - left)..];
            first = false;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::io::BufWriter;

    use super::*;

    #[test]
    fn basics() {
        let ical = "BEGIN:VCALENDAR
BEGIN:VEVENT
SUMMARY:foo bar
 test with
  multiple
  lines
END:VEVENT

END:VCALENDAR";

        let mut reader = LineReader::new(ical.as_bytes());
        assert_eq!(reader.next(), Some("BEGIN:VCALENDAR".to_string()));
        assert_eq!(reader.next(), Some("BEGIN:VEVENT".to_string()));
        assert_eq!(
            reader.next(),
            Some("SUMMARY:foo bartest with multiple lines".to_string())
        );
        assert_eq!(reader.next(), Some("END:VEVENT".to_string()));
        assert_eq!(reader.next(), Some("END:VCALENDAR".to_string()));
        assert_eq!(reader.next(), None);

        let res = Vec::new();
        let mut buf_writer = BufWriter::new(res);
        let mut writer = LineWriter::new(&mut buf_writer);
        let reader = LineReader::new(ical.as_bytes());
        for line in reader {
            writer.write_line(&line).unwrap();
        }

        let expected_ical = "BEGIN:VCALENDAR\r
BEGIN:VEVENT\r
SUMMARY:foo bartest with multiple lines\r
END:VEVENT\r
END:VCALENDAR\r
";
        assert_eq!(
            String::from_utf8(buf_writer.into_inner().unwrap()).unwrap(),
            expected_ical
        );
    }

    #[test]
    fn long_lines() {
        let ical = "BEGIN:VCALENDAR\r
TEST:0123456789012345678901234567890123456789012345678901234567890123456789\r
 01234567890123456789\r
END:VCALENDAR\r
";

        let mut reader = LineReader::new(ical.as_bytes());
        assert_eq!(reader.next(), Some("BEGIN:VCALENDAR".to_string()));
        assert_eq!(
            reader.next(),
            Some("TEST:012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789".to_string())
        );
        assert_eq!(reader.next(), Some("END:VCALENDAR".to_string()));
        assert_eq!(reader.next(), None);

        let res = Vec::new();
        let mut buf_writer = BufWriter::new(res);
        let mut writer = LineWriter::new(&mut buf_writer);
        let reader = LineReader::new(ical.as_bytes());
        for line in reader {
            writer.write_line(&line).unwrap();
        }

        assert_eq!(
            String::from_utf8(buf_writer.into_inner().unwrap()).unwrap(),
            ical
        );
    }

    #[test]
    fn more_props() {
        let att_str = "ATTENDEE;ROLE=REQ-PARTICIPANT;PARTSTAT=TENTATIVE;CN=Henry
  Cabot:mailto:hcabot@example.com";
        let mut reader = LineReader::new(att_str.as_bytes());
        assert_eq!(
            reader.next(),
            Some("ATTENDEE;ROLE=REQ-PARTICIPANT;PARTSTAT=TENTATIVE;CN=Henry Cabot:mailto:hcabot@example.com".to_string())
        );
    }
}
