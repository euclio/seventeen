use std::mem;

use log::*;

use protocol::{self, OpKind, Update};
use terminal::Coordinate;

#[derive(Debug, Clone, Default)]
pub struct LineCache {
    invalid_before: u64,
    lines: Vec<Line>,
    invalid_after: u64,
}

#[derive(Debug, Clone, Default)]
pub struct Line {
    pub(crate) text: String,
    pub(crate) cursors: Option<Vec<u64>>,
    pub(crate) styles: Vec<Style>,
}

#[derive(Debug, Clone)]
pub(crate) struct Style {
    start_index: i64,
    length: u64,
    style_id: u64,
}

impl LineCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// True if a given terminal coordinate is at the end of a line in the cache.
    pub fn is_eol(&self, coordinate: &Coordinate) -> bool {
        let x = coordinate.x as usize - 1;
        let y = coordinate.y as usize - 1;
        self.lines
            .iter()
            .nth(y)
            .and_then(|line| line.text.chars().nth(x))
            .map(|c| c == '\n')
            .unwrap_or_default()
    }

    /// Returns the text of last line in the cache, if present.
    ///
    /// Returns `None` if the cache is empty or there are invalid lines at the end of the cache.
    pub fn last_line(&self) -> Option<&str> {
        if self.invalid_after == 0 {
            self.lines.last().map(|line| line.text.as_str())
        } else {
            None
        }
    }

    fn ins(&mut self, lines: Vec<protocol::Line>) {
        debug!("inserting {} lines", lines.len());
        self.lines.extend(lines.into_iter().map(|line| {
            Line {
                text: line.text.expect("attempted to insert line with no text"),
                cursors: line.cursor,
                styles: vec![], // FIXME: Unimplemented
            }
        }));
    }

    fn invalidate(&mut self, n: u64) {
        debug!("appending {} invalid lines", n);
        // Invalid lines will only be added at the beginning or the end.
        if self.lines.is_empty() {
            self.invalid_before += n;
        } else {
            self.invalid_after += n;
        }
    }

    fn copy(&mut self, mut n: u64, old_cache: &mut Self) {
        debug!("copying {} lines", n);
        if n > 0 && old_cache.invalid_before > 0 {
            let num_invalid = if old_cache.invalid_before > n {
                old_cache.invalid_before -= n;
                n
            } else {
                mem::replace(&mut old_cache.invalid_before, 0)
            };

            self.invalidate(num_invalid);
            n -= num_invalid;
        }

        if n > 0 && !old_cache.lines.is_empty() {
            let lines = if old_cache.lines.len() as u64 > n {
                old_cache.lines.drain(..n as usize)
            } else {
                old_cache.lines.drain(..)
            };

            self.lines.extend(lines);
        }
    }

    fn skip(&mut self, mut n: u64) {
        debug!("skipping {} lines", n);

        // TODO: Notice that this is basically the same as copy, but it discards the old lines.
        if n > 0 && self.invalid_before > 0 {
            let num_invalid = if self.invalid_before > n {
                self.invalid_before -= n;
                n
            } else {
                mem::replace(&mut self.invalid_before, 0)
            };

            n -= num_invalid;
        }

        if n > 0 && !self.lines.is_empty() {
            if self.lines.len() as u64 > n {
                self.lines.drain(..n as usize);
            } else {
                self.lines.drain(..);
            }
        }
    }

    pub fn update(&mut self, update: Update) {
        // Semantically, this method simply replaces the old state of the cache with a new state.
        // Here we use a naive implementation to simply construct a new cache by applying the
        // update operations in order and draining from the old state where necessary.
        //
        // It's not so efficient, but it's probably good enough for now.

        let mut old_cache = LineCache::new();
        mem::swap(self, &mut old_cache);

        for op in update.ops {
            match op.op {
                OpKind::Ins => self.ins(op.lines.expect("attempted `ins` with no lines")),
                OpKind::Invalidate => self.invalidate(op.n),
                OpKind::Copy => self.copy(op.n, &mut old_cache),
                OpKind::Skip => old_cache.skip(op.n),
                _ => unimplemented!("unsupported op kind: {:?}", op),
            }
        }
    }

    pub fn iter_lines(&self) -> impl Iterator<Item = &str> {
        self.lines.iter().map(|line| line.text.as_str())
    }

    pub fn len(&self) -> usize {
        self.invalid_before as usize + self.lines.len() + self.invalid_after as usize
    }

    /// Returns the position of each cursor.
    pub fn iter_cursors<'a>(&'a self) -> impl Iterator<Item = Coordinate> + 'a {
        self.lines.iter().enumerate().flat_map(|(line_no, line)| {
            line.cursors.iter().flat_map(move |cursors| {
                cursors.iter().map(move |idx| Coordinate {
                    x: *idx as u16 + 1,
                    y: line_no as u16 + 1,
                })
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use protocol::{Line, Op, OpKind, Update};
    use terminal::Coordinate;

    use super::LineCache;

    #[test]
    fn insert() {
        let mut cache = LineCache::new();
        cache.update(Update {
            rev: None,
            ops: vec![Op {
                op: OpKind::Ins,
                n: 2,
                lines: Some(vec![
                    Line {
                        text: Some(String::from("Hello, world!")),
                        cursor: Some(vec![]),
                        styles: Some(vec![]),
                    },
                    Line {
                        text: Some(String::from("Goodbye, world!")),
                        cursor: Some(vec![]),
                        styles: Some(vec![]),
                    },
                ]),
            }],
            pristine: true,
        });

        assert_eq!(
            cache
                .lines
                .into_iter()
                .map(|line| line.text)
                .collect::<Vec<_>>(),
            vec!["Hello, world!", "Goodbye, world!"]
        );
    }

    #[test]
    fn invalidate() {
        let mut cache = LineCache::new();
        cache.update(Update {
            rev: None,
            ops: vec![Op {
                op: OpKind::Invalidate,
                n: 10,
                lines: None,
            }],
            pristine: true,
        });

        assert_eq!(cache.len(), 10);
        assert!(cache.lines.is_empty());
    }

    #[test]
    fn copy() {
        let mut cache = LineCache::new();
        cache.lines = vec![
            super::Line {
                text: String::from("Hello, world!"),
                ..Default::default()
            },
            super::Line {
                text: String::from("Goodbye, world!"),
                ..Default::default()
            },
        ];

        cache.update(Update {
            rev: None,
            ops: vec![Op {
                op: OpKind::Copy,
                n: 1,
                lines: None,
            }],
            pristine: true,
        });

        assert_eq!(cache.len(), 1);
        assert_eq!(
            cache.lines.into_iter().next().unwrap().text,
            "Hello, world!"
        );
    }

    #[test]
    fn copy_invalid_before() {
        let mut cache = LineCache::new();
        cache.invalid_before = 1;
        cache.lines = vec![
            super::Line {
                text: String::from("Hello, world!"),
                cursors: None,
                styles: vec![],
            },
            super::Line {
                text: String::from("Goodbye, world!"),
                cursors: None,
                styles: vec![],
            },
        ];

        cache.update(Update {
            rev: None,
            ops: vec![Op {
                op: OpKind::Copy,
                n: 2,
                lines: None,
            }],
            pristine: true,
        });

        assert_eq!(cache.len(), 2);
        assert_eq!(cache.invalid_before, 1);
        assert_eq!(cache.lines.len(), 1);
        assert_eq!(
            cache.lines.into_iter().next().unwrap().text,
            "Hello, world!"
        );
    }

    // TODO: Write test for invalid after?

    #[test]
    fn is_eol() {
        let mut cache = LineCache::new();
        cache.lines = vec![super::Line {
            text: String::from("Hello, world!\n"),
            ..Default::default()
        }];

        assert!(cache.is_eol(&Coordinate { y: 1, x: 14 }));
        assert!(!cache.is_eol(&Coordinate { y: 1, x: 10 }));
    }
}
