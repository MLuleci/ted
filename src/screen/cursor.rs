use crate::buffer::Buffer;
use crate::buffer::line::{Line, ColumnIndex};
use unicode_segmentation::GraphemeCursor;
use unicode_width::UnicodeWidthStr;
use std::cmp::min;

pub enum Direction {
    Up,
    Down,
    Left,
    Right
}

#[derive(Clone)]
pub struct Cursor {
    pub row: usize, // Line index
    pub column: usize, // Column index (visible)
    pub byte: usize, // Byte offset (in current line)
    pub index: usize, // Grapheme index
    pub offset: usize, // Byte offset (in buffer)
    desired_column: usize // Column index (actual)
}

impl Cursor {
    pub fn new() -> Self {
        Cursor {
            row: 0,
            column: 0,
            byte: 0,
            index: 0,
            offset: 0,
            desired_column: 0
        }
    }

    pub fn from(buf: &Buffer, x: usize, y: usize) -> Self {
        let line = buf.line(y).expect("No such line");
        let index = Cursor::find_column(line, x);
        let offset = Cursor::offset(y, buf) + index.byte;
        Cursor {
            row: y,
            column: index.column,
            byte: index.byte,
            index: index.index,
            offset,
            desired_column: 0
        }
    }

    fn find<'a, T>(line: &'a Line, f: T) -> ColumnIndex<'a>
        where T : Fn(&ColumnIndex) -> bool 
    {
        let mut previous = ColumnIndex {
            byte: 0,
            width: 0,
            column: 0,
            index: 0,
            grapheme: ""
        };

        for i in line.column_indices() {
            if f(&i) {
                return i;
            }
            previous = i;
        }

        return previous;
    }
    
    fn get_last_index(line: &Line) -> ColumnIndex {
        ColumnIndex {
            byte: line.text.len(),
            width: 0,
            column: line.width,
            index: line.size,
            grapheme: ""
        }
    }

    fn find_column(line: &Line, column: usize) -> ColumnIndex {
        if column >= line.width {
            return Cursor::get_last_index(line);
        }
        Cursor::find(line, |i| i.column <= column && column < i.column + i.width)
    }

    fn find_index(line: &Line, index: usize) -> ColumnIndex {
        if index >= line.size {
            return Cursor::get_last_index(line);
        }
        Cursor::find(line, |i| i.index == index)
    }

    fn check_bounds(&self, buf: &Buffer) {
        let line_count = buf.line_count();
        assert!(self.row < line_count, "Row out-of-bounds");

        let line = buf.line(self.row).unwrap();
        assert!(self.column <= line.width, "Column out-of-bounds");
        assert!(self.byte <= line.text.len(), "Offset out-of-bounds");
        assert!(self.index <= line.size, "Index out-of-bounds");
    }

    pub fn move_cursor(&mut self, buf: &Buffer, direction: Direction, steps: usize) {
        match direction {
            Direction::Up => {
                if steps > self.row {
                    // Goto start of first line
                    self.row = 0;
                    self.byte = 0;
                    self.index = 0;
                    self.column = 0;
                } else {
                    // Go up `steps` lines
                    self.row -= steps;

                    let line = buf.line(self.row).unwrap();
                    let index = Cursor::find_column(line, self.desired_column);
                    self.column = index.column;
                    self.byte = index.byte;
                    self.index = index.index;
                }
            },
            Direction::Down => {
                let line_count = buf.line_count();
                if steps + self.row >= line_count {
                    // Goto end of last line
                    self.row = line_count - 1;
                    let line = buf.line(self.row).unwrap();
                    self.byte = line.text.len();
                    self.index = line.size;
                    self.column = line.width;
                } else {
                    // Go down `steps` lines
                    self.row += steps;

                    let line = buf.line(self.row).unwrap();
                    let index = Cursor::find_column(line, self.desired_column);
                    self.column = index.column;
                    self.byte = index.byte;
                    self.index = index.index;
                }
            },
            Direction::Left => {
                // Find the row and index after moving `steps` to the left
                let mut remain = steps;
                while remain > 0 {
                    let take = min(remain, self.index);
                    self.index -= take;
                    remain -= take;

                    if self.index <= 0 && remain > 0 {
                        if self.row == 0 {
                            break;
                        } else {
                            self.row -= 1;
                            let line = buf.line(self.row).unwrap();
                            self.index = line.size;
                            remain -= 1;
                        }
                    }
                }

                let line = buf.line(self.row).unwrap();
                let index = Cursor::find_index(line, self.index);
                self.column = index.column;
                self.byte = index.byte;
                self.desired_column = index.column;
            },
            Direction::Right => {
                // Find the row and index after moving `steps` to the right
                let mut remain = steps;
                let line_count = buf.line_count();
                while remain > 0 {
                    let line = buf.line(self.row).unwrap();
                    let take = min(remain, line.size - self.index);
                    self.index += take;
                    remain -= take;

                    if self.index >= line.size && remain > 0 {
                        if self.row >= line_count - 1 {
                            break;
                        } else {
                            self.row += 1;
                            self.index = 0;
                            remain -= 1;
                        }
                    }
                }

                let line = buf.line(self.row).unwrap();
                let index = Cursor::find_index(line, self.index);
                self.column = index.column;
                self.byte = index.byte;
                self.desired_column = index.column;
            }
        }

        self.offset = Cursor::offset(self.row, buf) + self.byte;
        self.check_bounds(buf);
    }

    // Version of `move_cursor` optimized for stepping left/right by one character
    pub fn step_cursor(&mut self, buf: &Buffer, direction: Direction) {
        match direction {
            Direction::Left => {
                let line = buf.line(self.row).unwrap();
                let mut cursor = GraphemeCursor::new(self.byte, line.text.len(), true);
                match cursor.prev_boundary(&line.text, 0) {
                    Ok(Some(previous)) => {
                        // Step left by one character
                        let s = &line.text[previous..self.byte];
                        self.offset -= self.byte - previous;
                        self.column -= s.width_cjk();
                        self.byte = previous;
                        self.index -= 1;
                        self.desired_column = self.column;
                    },
                    Ok(None) => {
                        if self.row > 0 {
                            // Go to end of previous line
                            self.row -= 1;
                            self.end(buf);
                        } else {
                            // Go to start of first line
                            self.home(buf);
                        }
                    },
                    Err(_) => panic!("Incomplete chunk - step left")
                }
            },
            Direction::Right => {
                let line = buf.line(self.row).unwrap();
                let line_count = buf.line_count();
                let mut cursor = GraphemeCursor::new(self.byte, line.text.len(), true);
                match cursor.next_boundary(&line.text, 0) {
                    Ok(Some(next)) => {
                        // Step right by one character
                        let s = &line.text[self.byte..next];
                        self.offset += next - self.byte;
                        self.column += s.width_cjk();
                        self.byte = next;
                        self.index += 1;
                        self.desired_column = self.column;
                    },
                    Ok(None) => {
                        if self.row < line_count - 1 {
                            // Go to start of next line
                            self.row += 1;
                            self.home(buf);
                        } else {
                            // Go to end of last line
                            self.end(buf);
                        }
                    },
                    Err(_) => panic!("Incomplete chunk - step right")
                }
            }
            _ => self.move_cursor(buf, direction, 1)
        }

        self.check_bounds(buf);
    }

    pub fn home(&mut self, buf: &Buffer) {
        self.column = 0;
        self.byte = 0;
        self.index = 0;
        self.offset = Cursor::offset(self.row, buf);
        self.desired_column = 0;
    }

    pub fn end(&mut self, buf: &Buffer) {
        let line = buf.line(self.row).unwrap();
        self.column = line.width;
        self.byte = line.text.len();
        self.index = line.size;
        self.offset = Cursor::offset(self.row, buf) + self.byte;
        self.desired_column = self.column;
    }

    pub fn top(&mut self, buf: &Buffer) {
        self.row = 0;
        self.home(buf);
    }

    pub fn bottom(&mut self, buf: &Buffer) {
        self.row = buf.line_count() - 1;
        self.end(buf);
    }

    fn offset(row: usize, buf: &Buffer) -> usize {
        buf.lines().iter().take(row)
            .fold(0, |acc, i| acc + i.text.len())
    }
}