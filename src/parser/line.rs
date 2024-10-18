use std::io::BufRead;

pub struct LineReader<R: BufRead> {
    reader: R,
    last: Option<String>,
}

impl<R: BufRead> LineReader<R> {
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

#[cfg(test)]
mod tests {
    use super::LineReader;

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
    }
}
