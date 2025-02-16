use std::{iter::Enumerate, ops::Bound};
use unicode_segmentation::{GraphemeIndices, UnicodeSegmentation};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
use std::ops::RangeBounds;

pub struct ColumnIndices<'a> {
    iter: Enumerate<GraphemeIndices<'a>>,
    column: usize
}

pub struct ColumnIndex<'a> {
    pub byte: usize, // byte offset
    pub width: usize, // column width
    pub column: usize,
    pub index: usize, // grapheme index
    pub grapheme: &'a str,
}

impl<'a> Iterator for ColumnIndices<'a> {
    type Item = ColumnIndex<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some((index, (offset, grapheme))) = self.iter.next() {
            let column = self.column;
            let width = grapheme.width_cjk(); 
            self.column += width;
            return Some(
                ColumnIndex {
                    byte: offset,
                    width,
                    column,
                    index,
                    grapheme
                }
            )
        }
        None
    }
}

#[derive(Clone)]
pub struct Line {
    pub text: String,
    pub size: usize, // Number of graphemes
    pub width: usize // Number of columns
}

impl Line {
    pub fn new() -> Self {
        Line {
            text: String::new(),
            size: 0,
            width: 0,
        }
    }

    pub fn from(s: &str) -> Self {
        Line {
            text: String::from(s),
            size: s.graphemes(true).count(),
            width: s.width_cjk()
        }
    }

    pub fn column_indices(&self) -> ColumnIndices {
        ColumnIndices {
            iter: self.text.grapheme_indices(true).enumerate(),
            column: 0
        }
    }

    pub fn insert(&mut self, c: char, i: usize) {
        let width = c.width_cjk().unwrap_or(0);
        if width > 0 {
            self.text.insert(i, c);
            self.width += width;
            self.size += 1;
        }
    }

    pub fn insert_str(&mut self, s: &str, i: usize) {
        self.text.insert_str(i, s);
        self.width += s.width_cjk();
        self.size += s.graphemes(true).count();
    }

    pub fn delete<R>(&mut self, i: R) -> String
        where R : RangeBounds<usize> 
    {
        let s: String = self.text.drain(i).collect();
        self.width -= s.width_cjk();
        self.size -= s.graphemes(true).count();
        s
    }

    pub fn clear(&mut self) -> String {
        let s = std::mem::take(&mut self.text);
        self.width = 0;
        self.size = 0;
        s
    }

    pub fn concat(&mut self, other: &Self) {
        self.text.push_str(&other.text);
        self.width += other.width;
        self.size += other.size;
    }

    pub fn concat_str(&mut self, s: &String) {
        self.text.push_str(s);
        self.width += s.width_cjk();
        self.size += s.graphemes(true).count();
    }

    pub fn split(&mut self, i: usize) -> Self {
        let s = self.text.split_off(i);
        let width = s.width_cjk();
        let size = s.graphemes(true).count();  
        self.width -= width;
        self.size -= size;
        Line { text: s, width, size }
    }

    pub fn replace<R>(&mut self, c: char, i: R) -> String
        where R : RangeBounds<usize> 
    {
        let start = match i.start_bound() {
            Bound::Included(&x) => x,
            Bound::Excluded(&x) => x,
            Bound::Unbounded => 0
        };
        let p = self.delete(i);
        self.insert(c, start);
        return p;
    }

    pub fn replace_str<R>(&mut self, s: &str, i: R) -> String
        where R : RangeBounds<usize> 
    {
        let start = match i.start_bound() {
            Bound::Included(&x) => x,
            Bound::Excluded(&x) => x,
            Bound::Unbounded => 0
        };
        let p = self.delete(i);
        self.insert_str(s, start);
        return p;
    }
}