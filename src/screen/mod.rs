pub mod cursor;

use cursor::{Cursor, Direction};
use termion::event::{Event, Key};
use unicode_width::UnicodeWidthStr;
use crate::buffer::{Buffer, Edit, Point};
use crate::Config;
use termion as t;
use std::io::{self, Write};
use std::cmp::min;
use std::path::Path;

const LINE_HIGHLIGHT: t::color::Rgb = t::color::Rgb(39, 39, 39);
const LINE_TEXT: t::color::LightWhite = t::color::LightWhite;
const STATUS_HIGHLIGHT: t::color::Rgb = t::color::Rgb(184, 184, 184);
const STATUS_TEXT: t::color::White = t::color::White;

pub enum Message {
    Info(String),
    Warning(String),
    Error(String)
}

impl Message {
    fn content(&self) -> &String {
        match self {
            Message::Info(s) => s,
            Message::Warning(s) => s,
            Message::Error(s) => s
        }
    }
    
    fn set_color(&self, out: &mut impl Write) -> io::Result<()> {
        match self {
            Message::Info(_) =>
                write!(out, "{}{}", 
                    t::color::Bg(STATUS_HIGHLIGHT),
                    t::color::Fg(STATUS_TEXT)
                ),
            Message::Warning(_) => 
                write!(out, "{}{}", 
                    t::color::Bg(t::color::Rgb(230, 150, 0)),
                    t::color::Fg(t::color::White)
                ),
            Message::Error(_) => 
                write!(out, "{}{}",
                    t::color::Bg(t::color::Rgb(200, 0, 0)),
                    t::color::Fg(t::color::White)
                )
        }
    }
}

pub struct Screen {
    buffer: Buffer,
    origin: Point, // Top-left edge of the viewport, in rows and columns
    cursor: Cursor,
    pub overwrite: bool,
    message: Option<Message>,
    undo_stack: Vec<(Cursor, Edit)>,
    redo_stack: Vec<(Cursor, Edit)>
}

impl Screen {
    pub fn new(path: &str, config: &Config) -> Self {
        let mut message: Option<Message> = None;
        let buffer = Buffer::build(path, &config)
            .unwrap_or_else(|e| {
                message = Some(Message::Error(e.to_string()));
                Buffer::new(path, &config)
            });

        Screen {
            buffer,
            origin: Point::new(),
            cursor: Cursor::new(),
            overwrite: false,
            message,
            undo_stack: Vec::new(),
            redo_stack: Vec::new()
        }
    }
    
    pub fn draw<T>(&mut self, out: &mut T) -> io::Result<()> where T : Write {
        self.update_viewport();
        let number_width = self.line_number_width();
        let (width, height) = self.get_viewport_size();

        write!(out, "{}", t::clear::All)?;

        let lines = self.buffer.lines()
            .iter()
            .skip(self.origin.y)
            .take(height)
            .enumerate();

        for (i, line) in lines {
            let x = self.origin.x;
            let y = self.origin.y + i;

            // Setup colors:
            if self.cursor.row == y {
                write!(out, "{}{}", t::color::Bg(LINE_HIGHLIGHT), t::color::Fg(LINE_TEXT))?;
            } else {
                write!(out, "{}", t::color::Fg(LINE_HIGHLIGHT))?;
            }

            // Print line number:
            let mut printed = 0;
            let position = t::cursor::Goto(1, (i + 1) as u16);
            write!(out, "{}{:>number_width$} ", position, y + 1)?;

            if self.cursor.row != y {
                write!(out, "{}", t::color::Fg(t::color::Reset))?;
            }

            let mut iter = line.column_indices();
            match iter.find(|c| c.column <= x && x < c.column + c.width)
            {
                None => (), // Line is not visible in viewport
                Some(start) => {
                    let mut first = start.offset;
                    if start.column < x {
                        // First character is partially visible, pad the start
                        let space = (start.column + start.width) - x;
                        write!(out, "{:-<space$}", "<")?;
                        first += start.grapheme.len();
                    }

                    match iter.find(|c| c.column <= x + width && x + width < c.column + c.width)
                    {
                        Some(end) => {
                            if end.column + end.width > x + width {
                                // Last character is partially visible, pad the end
                                let space = (x + width) - end.column;
                                
                                write!(out, "{}", &line.text[first..end.offset])?; // Print all but last character
                                write!(out, "{:->space$}", ">")?; // Print padding
                            } else {
                                // Last character is visible, print the whole line
                                write!(out, "{}", &line.text[first..])?;
                            }
                            printed = end.column - start.column;
                        },
                        None => {
                            // Line doesn't collide with right edge, print it whole
                            write!(out, "{}", &line.text[first..])?;
                            printed = line.width - start.column;
                        }
                    }
                }
            }

            // Finish coloring the rest of the row:
            if self.cursor.row == y {
                let remaining = width - printed;
                write!(out, "{:remaining$}{}{}", "", t::color::Bg(t::color::Reset), t::color::Fg(t::color::Reset))?;
            }
        }

        // Draw status line:
        let (width, height) = t::terminal_size().unwrap();
        write!(out, "{}", t::cursor::Goto(1, height))?;

        if let Some(m) = &self.message {
            let s = m.content();
            let pad = width as usize - 1;
            m.set_color(out)?;
            write!(out, " {:<pad$}", s)?;
        } else {
            write!(out, "{}{}",
                t::color::Bg(STATUS_HIGHLIGHT),
                t::color::Fg(STATUS_TEXT)
            )?;

            let path = self.buffer.path()
                .file_name()
                .map_or(
                    "[new buffer]", 
                    |i| i.to_str().expect("path is not valid unicode")
                );
            let rhs = format!("{} ({}, {}) {}", 
                if self.overwrite { "INS" } else { "" },
                self.cursor.row, 
                self.cursor.column, 
                self.buffer.line_ending()
            );
            let pad = width as usize - path.width_cjk() - 3;
            write!(out, " {} {:>pad$} ", path, rhs)?;
        }

        write!(out, "{}{}",
            t::color::Bg(t::color::Reset),
            t::color::Fg(t::color::Reset),
        )?;

        // Draw cursor:
        let x = (self.cursor.column - self.origin.x + number_width) as u16 + 2;
        let y = (self.cursor.row - self.origin.y) as u16 + 1;
        let position = t::cursor::Goto(x, y);
        if self.overwrite {
            write!(out, "{}", t::cursor::BlinkingBlock)?;
        } else {
            write!(out, "{}", t::cursor::BlinkingBar)?;
        }
        write!(out, "{}", position)?;

        Ok(())
    }
    
    pub fn prompt<T, I>(&self, events: &mut I, out: &mut T, prompt: &str) 
        -> io::Result<Option<String>>
        where T : Write
            , I : Iterator<Item = io::Result<Event>>
    {
        let mut buffer = String::new();
        let prompt_width = prompt.width_cjk();
        write!(out, "{}", t::cursor::BlinkingUnderline)?;

        loop {
            let (width, height) = t::terminal_size().unwrap();
            let pad = width as usize - prompt_width - 3;
            let end = prompt_width + buffer.width_cjk() + 3;
            
            write!(out, "{}{}{} {} {:<pad$} {}{}{}",
                t::cursor::Goto(1, height),
                t::color::Bg(STATUS_HIGHLIGHT),
                t::color::Fg(STATUS_TEXT),
                prompt,
                buffer,
                t::color::Bg(t::color::Reset),
                t::color::Fg(t::color::Reset),
                t::cursor::Goto(end as u16, height)
            )?;
            out.flush()?;

            if let Some(event) = events.next() {
                match event? {
                    Event::Key(Key::Esc) => break,
                    Event::Key(Key::Char(ch)) => {
                        match ch {
                            '\n' => return Ok(Some(buffer)),
                            _ => buffer.push(ch),
                        }
                    },
                    Event::Key(Key::Backspace) => { buffer.pop(); },
                    _ => continue
                }
            }
        }

        Ok(None)
    }

    pub fn confirm_prompt<T, I>(&self, events: &mut I, out: &mut T, prompt: &str, default: bool) 
    -> io::Result<bool>
    where T : Write
        , I : Iterator<Item = io::Result<Event>>
    {
        Ok(self.prompt(events, out, prompt)?
            .and_then(|i| i
                .chars()
                .next()
                .map(|c| c.to_ascii_lowercase() == 'y')
            )
            .unwrap_or(default))
    }

    fn line_number_width(&self) -> usize {
        // `ilog10` may panic if length = 0, but this should never be true,
        // `as usize` may panic if `usize` isn't big enough to contain a `u32`,
        // but even if we compute the number of digits using strings, we can
        // at most count up to `usize::MAX`
        let length = self.buffer.line_count();
        assert_ne!(length, 0);
        length.ilog10() as usize + 1
    }

    fn get_viewport_size(&self) -> (usize, usize) {
        let (width, height) = t::terminal_size()
            .expect("Failed to get terminal size");

        // `+1` is for the space between numbers and text
        let number_width = self.line_number_width() + 1;

        (width as usize - number_width, height as usize - 1)
    }

    fn update_viewport(&mut self) {
        let (mut origin_x, mut origin_y) = self.origin.as_tuple();
        let (width, height) = self.get_viewport_size();
        let cursor_y = self.cursor.row;
        let cursor_x = self.cursor.column;

        if cursor_y >= origin_y && (cursor_y - origin_y) >= height {
            // Move `top` down to keep cursor visible
            origin_y = cursor_y - height + 1;
        } else if cursor_y < origin_y {
            // Move `top` up to the cursor
            origin_y = cursor_y;
        }

        let padding = 4;
        let padded_width = if width >= padding { width - padding } else { width };
        let line = self.buffer.line(cursor_y).unwrap();
        let column = min(cursor_x, line.width);

        if column >= origin_x && (column - origin_x) >= padded_width {
            // Move `left` right to keep cursor visible (w/ padding)
            origin_x = column - padded_width + 1;
        } else if column <= origin_x + padding {
            // Move `left` left to padded position (or clip to zero)
            origin_x = if column >= padding { column - padding } else { 0 };
        }

        // Assert: cursor is visible
        assert!(cursor_y >= origin_y && (cursor_y - origin_y) < height);
        assert!(column >= origin_x && (column - origin_x) < width);

        // self.redraw |= origin_x != self.origin.x || origin_y != self.origin.y;
        self.origin = Point { x: origin_x, y: origin_y };
    }

    pub fn move_cursor(&mut self, direction: Direction) {
        self.cursor.step_cursor(&self.buffer, direction);
    }

    pub fn set_cursor(&mut self, x: usize,  y: usize) {
        let x = x - self.line_number_width() + self.origin.x;

        let line_count = self.buffer.line_count();
        assert_ne!(line_count, 0, "Buffer is empty!");

        let y = min(y + self.origin.y, line_count - 1);

        self.cursor = Cursor::from(&self.buffer, x, y);
    }

    fn push_undo(&mut self, item: (Cursor, Edit)) {
        self.redo_stack.clear();
        self.undo_stack.push(item);
    }

    pub fn insert(&mut self, ch: char) {
        let pt = Point { x: self.cursor.offset, y: self.cursor.row };
        let edit = Edit::Insert(ch, pt);

        if let Some(undo) = self.buffer.execute(&edit) {
            let before = self.cursor.clone();
            self.cursor.step_cursor(&self.buffer, Direction::Right);
            self.push_undo((before, undo));
        }
    }

    pub fn overwrite(&mut self, ch: char) {
        let pt = Point { x: self.cursor.offset, y: self.cursor.row };
        let edit = Edit::Overwrite(ch, pt);

        if let Some(undo) = self.buffer.execute(&edit) {
            let before = self.cursor.clone();
            self.cursor.step_cursor(&self.buffer, Direction::Right);
            
            self.push_undo((before, undo));
        }
    }

    pub fn backspace(&mut self) {
        let before = self.cursor.clone();
        self.cursor.step_cursor(&self.buffer, Direction::Left);

        let pt = Point { x: self.cursor.offset, y: self.cursor.row };
        let edit = Edit::Delete(pt);

        if let Some(undo) = self.buffer.execute(&edit) {
            self.push_undo((before, undo));
        }
    }

    pub fn delete(&mut self) {
        let pt = Point { x: self.cursor.offset, y: self.cursor.row };
        let edit = Edit::Delete(pt);

        if let Some(undo) = self.buffer.execute(&edit) {
            let before = self.cursor.clone();
            self.push_undo((before, undo));
        }
    }

    pub fn home(&mut self) {
        self.cursor.home();
    }

    pub fn end(&mut self) {
        self.cursor.end(&self.buffer);
    }

    pub fn top(&mut self) {
        self.cursor.top();
    }

    pub fn bottom(&mut self) {
        self.cursor.bottom(&self.buffer);
    }

    pub fn undo(&mut self) {
        if let Some((_, last)) = self.undo_stack.last() {
            let kind = std::mem::discriminant(last);

            while !self.undo_stack.is_empty() {
                let (_, u) = self.undo_stack.last().unwrap();
                if std::mem::discriminant(u) != kind { break; }

                let (cursor, undo) = self.undo_stack.pop().unwrap();
                if let Some(redo) = self.buffer.execute(&undo) {
                    self.redo_stack.push((self.cursor.clone(), redo));
                    self.cursor = cursor;
                } else {
                    break; // Failed to execute undo
                }
            }
        }
    }

    pub fn redo(&mut self) {
        if let Some((_, last)) = self.redo_stack.last() {
            let kind = std::mem::discriminant(last);

            while !self.redo_stack.is_empty() {
                let (_, r) = self.redo_stack.last().unwrap();
                if std::mem::discriminant(r) != kind { break; }

                let (cursor, redo) = self.redo_stack.pop().unwrap();
                if let Some(undo) = self.buffer.execute(&redo) {
                    self.undo_stack.push((self.cursor.clone(), undo));
                    self.cursor = cursor;
                } else {
                    break; // Failed to execute redo
                }
            }
        }
    }

    pub fn set_message(&mut self, m: Message) {
        self.message = Some(m)
    }

    pub fn clear_message(&mut self) {
        self.message = None
    }

    pub fn is_dirty(&self) -> bool {
        self.buffer.is_dirty()
    }

    pub fn save(&mut self, overwrite: bool) -> io::Result<usize> {
        self.buffer.save(overwrite)
    }

    pub fn save_as(&mut self, path: &Path, overwrite: bool) -> io::Result<usize> {
        self.buffer.save_as(&path, overwrite)
    }

    pub fn path(&self) -> &Path {
        self.buffer.path()
    }
}