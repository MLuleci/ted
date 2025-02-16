pub mod line;

use line::Line;
use crate::Config;
use unicode_segmentation::GraphemeCursor;
use std::cmp::min;
use std::fmt::Display;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::fs::OpenOptions;

#[derive(Clone)]
pub enum LineEnding { CRLF, LF }

impl LineEnding {
    fn value(&self) -> &'static str {
        match *self {
            Self::CRLF => "\r\n",
            Self::LF => "\n"
        }
    }

    #[cfg(target_os = "windows")]
    fn default() -> LineEnding {
        LineEnding::CRLF
    }

    #[cfg(not(target_os = "windows"))]
    fn default() -> LineEnding {
        LineEnding::LF
    }
}

impl Display for LineEnding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match *self {
            Self::CRLF => "CRLF",
            Self::LF => "LF"
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Point {
    pub x: usize,
    pub y: usize,
}

impl Point {
    pub fn new() -> Point { 
        Point { x: 0, y: 0 }
    }

    pub fn as_tuple(&self) -> (usize, usize) {
        (self.x, self.y)
    }
}

#[derive(Clone)]
pub enum Edit {
    Insert(char, Point),
    Overwrite(char, Point),
    Delete(Point),
    Paste(Point, String),
    Cut(Point, Point),
    Replace(Point, usize, String)
}

#[derive(Clone)]
pub struct Buffer {
    path: PathBuf,
    lines: Vec<Line>,
    modified: SystemTime,
    ending: LineEnding,
    dirty: bool,
    readonly: bool // Does the user want to be able to write to the file?
}

impl Buffer {
    pub fn new(path: &str, config: &Config) -> Self {
        Buffer {
            path: PathBuf::from(path),
            lines: vec![Line::new()],
            ending: LineEnding::default(),
            modified: SystemTime::now(),
            dirty: false,
            readonly: config.readonly
        }
    }

    pub fn build(path: &str, config: &Config) -> io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .truncate(config.truncate)
            .open(path);

        if let Err(e) = file {
            return match e.kind() {
                io::ErrorKind::NotFound => Ok(Buffer::new(path, config)),
                _ => Err(e)
            };
        }
        
        let file = file.unwrap();
        let metadata = file.metadata()?;
        let modified = metadata.modified()?;
        let mut reader = BufReader::new(file);
        let mut buffer = String::new();
        let mut lines = Vec::new();

        while BufRead::read_line(&mut reader, &mut buffer)? != 0 {
            lines.push(buffer.clone());
            buffer.clear();
        }

        let ending = match lines.first() {
            Some(l) => if l.ends_with("\r\n") { LineEnding::CRLF } else { LineEnding::LF },
            None => {
                lines.push(String::new()); // Initialize empty buffer
                LineEnding::default() // Empty or new file
            }
        };

        // Remove line endings:
        let lines: Vec<Line> = lines
            .iter()
            .map(|s| s.trim_end_matches(ending.value()))
            .map(Line::from)
            .collect();

        Ok(Buffer {
            path: PathBuf::from(path),
            lines,
            ending,
            modified,
            dirty: false,
            readonly: config.readonly 
        })
    }

    fn write_to(&self, path: &Path, overwrite: bool) -> io::Result<usize> {
        if self.readonly {
            return Err(io::Error::new(
                io::ErrorKind::ReadOnlyFilesystem,
                "Buffer is readonly"
            ));
        }

        if path.try_exists()? {
            let modified = path.metadata()?
                .modified()
                .unwrap_or(SystemTime::now());
            if modified > self.modified && !overwrite {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "File was modified"
                ));
            }
        }

        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .open(&path)?;

        let mut writer = BufWriter::new(&file);
        let data = self.to_string();
        let len = data.len();

        writer.write_all(data.as_bytes())
            .and_then(|_| file.set_len(len as u64))?;

        Ok(len)
    }

    pub fn save(&mut self, overwrite: bool) -> io::Result<usize> {
        self
            .write_to(&self.path, overwrite)
            .inspect(|_| {
                self.dirty = false;
                self.modified = SystemTime::now();
            })
    }

    pub fn save_as(&mut self, path: &Path, overwrite: bool) -> io::Result<usize> {
        if path.try_exists()? && !overwrite {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "Path already exists"
            ));
        }

        self
            .write_to(&path, overwrite)
            .inspect(|_| {
                self.dirty = false;
                self.modified = SystemTime::now();
                self.path = PathBuf::from(path);
            })
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn is_readonly(&self) -> bool {
        self.readonly
    }

    pub fn lines(&self) -> &Vec<Line> {
        &self.lines
    }

    pub fn line(&self, index: usize) -> Option<&Line> {
        self.lines.get(index)
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn line_ending(&self) -> &LineEnding {
        &self.ending
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn execute(&mut self, edit: &Edit) -> Option<Edit> {
        let undo: Option<Edit> = match edit {
            Edit::Insert(ch, pt) => {
                if let Some(line) = self.lines.get_mut(pt.y) {
                    if *ch == '\n' {
                        let tail = line.split(pt.x);
                        let index = pt.y + 1;
                        self.lines.insert(index, tail);
                        Some(Edit::Delete(Point { x: 0, y: index - 1 }))
                    } else {
                        line.insert(*ch, pt.x);
                        Some(Edit::Delete(pt.clone()))
                    }
                } else {
                    None
                }
            },
            Edit::Overwrite(ch, pt) => {
                if let Some(line) = self.lines.get_mut(pt.y) {
                    let mut cursor = GraphemeCursor::new(pt.x, line.text.len(), true);
                    match cursor.next_boundary(&line.text, 0) {
                        Ok(Some(next)) => {
                            // Overwrite some character in this line
                            let previous = line.replace(*ch, pt.x..next)
                                .chars()
                                .last()
                                .expect("No character returned");
                            Some(Edit::Overwrite(previous, pt.clone()))
                        },
                        Ok(None) => {
                            // Append to the end of the line
                            line.insert(*ch, line.text.len());
                            Some(Edit::Delete(pt.clone()))
                        },
                        Err(_) => panic!("Incomplete chunk - overwrite")
                    }
                } else {
                    None
                }
            },
            Edit::Delete(pt) => {
                if let Some(line) = self.lines.get(pt.y) {
                    let mut cursor = GraphemeCursor::new(pt.x, line.text.len(), true);
                    match cursor.next_boundary(&line.text, 0) {
                        Ok(Some(next)) => {
                            // Delete some character in this line
                            let line = self.lines.get_mut(pt.y).unwrap();
                            let ch = line.delete(pt.x..next)
                                .chars()
                                .last()
                                .expect("No character returned");
                            Some(Edit::Insert(ch, pt.clone()))
                        },
                        Ok(None) => { 
                            // Delete ending and join with next line
                            if pt.y < self.line_count() - 1 {
                                let next = self.lines.remove(pt.y + 1);
                                let line = self.lines.get_mut(pt.y).unwrap();
                                let len = line.text.len();
                                line.concat(&next);
                                Some(Edit::Insert('\n', Point { x: len, y: pt.y }))
                            } else {
                                None
                            }
                        },
                        Err(_) => panic!("Incomplete chunk - delete")
                    }
                } else {
                    None
                }
            },
            Edit::Cut(l, r) => {
                let mut buffer = String::new();
                let mut head = l.clone();

                // Cut parts of lines between `l` and `r`
                while head.y <= r.y {
                    if let Some(line) = self.lines.get_mut(head.y) {
                        let limit = if head.y != r.y { line.text.len() } else { r.x };
                        let take = limit - head.x;
                        let cut = if take >= line.text.len() {
                            line.clear()
                        } else {
                            line.delete(head.x..(head.x + take))
                        };
                        buffer.push_str(&cut);

                        if head.y < r.y { 
                            buffer.push_str(&self.ending.value());
                        }

                        head.x = 0;
                        head.y += 1;
                    } else { break }
                }

                if l.y != r.y {
                    // Concatenate first and last lines
                    let last = self.lines
                        .get_mut(r.y)
                        .map(|l| l.clear())
                        .unwrap_or_default();

                    if let Some(first) = self.lines.get_mut(l.y) {
                        first.concat_str(&last);
                    }

                    // Delete empty lines between `l` and `r`
                    for i in (l.y..=r.y).rev() {
                        if let Some(line) = self.lines.get(i) {
                            if line.text.is_empty() {
                                self.lines.remove(i);
                            }
                        }
                    }

                    if self.line_count() == 0 {
                        self.lines.push(Line::new());
                    }
                }
                
                Some(Edit::Paste(l.clone(), buffer))
            }
            _ => unimplemented!()
        };
        
        self.dirty |= undo.is_some();
        return undo;
    }
}

impl Display for Buffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (i, line) in self.lines.iter().enumerate() {
            write!(f, "{}", line.text)?;
            if i < self.lines.len() - 1 {
                write!(f, "{}", self.ending.value())?;
            }
        }
        
        Ok(())
    }
}

impl std::fmt::Debug for Buffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Buffer")
         .field("path", &self.path)
         .field("length", &self.lines.len())
         .field("ending", &self.ending.value())
         .field("modified", &self.modified)
         .field("dirty", &self.dirty)
         .field("readonly", &self.readonly)
         .finish()
    }
}