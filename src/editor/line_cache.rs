use std::mem;
use std::slice::Chunks;

use log::*;

use protocol::{self, OpKind, Update};
use screen::Coordinate;

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
    styles: Vec<i64>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct StyleSpan {
    pub id: u64,
    pub start: usize,
    pub length: usize,
}

impl Line {
    pub fn iter_style_spans<'a>(&'a self) -> impl Iterator<Item = StyleSpan> + 'a {
        struct StyleIterator<'a> {
            style_triples: Chunks<'a, i64>,
            last_span_end: usize,
        }

        impl<'a> Iterator for StyleIterator<'a> {
            type Item = StyleSpan;

            fn next(&mut self) -> Option<Self::Item> {
                self.style_triples.next().map(|triple| {
                    let start_index = triple[0];
                    let length = triple[1] as usize;
                    let id = triple[2] as u64;

                    let span_start = ((self.last_span_end as i64) + start_index) as usize;
                    self.last_span_end = span_start + length;
                    StyleSpan {
                        start: span_start,
                        length,
                        id,
                    }
                })
            }
        }

        StyleIterator {
            style_triples: self.styles.chunks(3),
            last_span_end: 0,
        }
    }
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

    #[cfg(test)]
    pub fn new_from_lines(lines: &'static [&'static str]) -> Self {
        LineCache {
            invalid_before: 0,
            lines: lines
                .iter()
                .map(|line| Line {
                    text: line.to_string(),
                    ..Default::default()
                })
                .collect(),
            invalid_after: 0,
        }
    }

    /// True if a given terminal coordinate is at the end of a line in the cache.
    pub fn is_eol(&self, coordinate: &Coordinate) -> bool {
        self.lines
            .iter()
            .nth(usize::from(coordinate.y))
            .and_then(|line| line.text.chars().nth(usize::from(coordinate.x)))
            .map(|c| c == '\n')
            .unwrap_or_default()
    }

    fn ins(&mut self, lines: Vec<protocol::Line>) {
        debug!("inserting {} lines", lines.len());
        self.lines.extend(lines.into_iter().map(|line| Line {
            text: line.text.expect("attempted to insert line with no text"),
            cursors: line.cursor,
            styles: line.styles.unwrap_or_default(),
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

    pub fn iter_lines(&self) -> impl Iterator<Item = &Line> {
        self.lines.iter().map(|line| line)
    }

    pub fn len(&self) -> usize {
        self.invalid_before as usize + self.lines.len() + self.invalid_after as usize
    }

    /// Returns the position of each cursor.
    pub fn iter_cursors<'a>(&'a self) -> impl Iterator<Item = Coordinate> + 'a {
        self.lines.iter().enumerate().flat_map(|(line_no, line)| {
            line.cursors.iter().flat_map(move |cursors| {
                cursors.iter().map(move |idx| Coordinate {
                    x: *idx as u16,
                    y: line_no as u16,
                })
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use protocol::{Line, Op, OpKind, Update};
    use screen::Coordinate;

    use super::{LineCache, StyleSpan};

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
                        ..Default::default()
                    },
                    Line {
                        text: Some(String::from("Goodbye, world!")),
                        ..Default::default()
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

        assert!(cache.is_eol(&Coordinate { y: 0, x: 13 }));
        assert!(!cache.is_eol(&Coordinate { y: 0, x: 10 }));
    }

    #[test]
    fn style_spans() {
        let line = super::Line {
            text: String::from("int main() {}\n"),
            cursors: Some(vec![0]),
            styles: vec![0, 3, 2, 0, 1, 3, 0, 4, 4, -1, 3, 2],
        };

        let spans = line.iter_style_spans().collect::<Vec<_>>();

        assert_eq!(
            spans,
            vec![
                StyleSpan {
                    start: 0,
                    length: 3,
                    id: 2,
                },
                StyleSpan {
                    start: 3,
                    length: 1,
                    id: 3,
                },
                StyleSpan {
                    start: 4,
                    length: 4,
                    id: 4,
                },
                StyleSpan {
                    start: 7,
                    length: 3,
                    id: 2,
                },
            ]
        );
    }
}
