//! Small helpers on top of `roxmltree` mirroring how the game reads its XML:
//! first-direct-child lookups, inner text, CSV integer lists, hex colours,
//! `True`/`False` booleans and the Ogmo bitstring / trimmed-CSV grids.

use crate::error::{Error, Result, read_text};
use roxmltree::{Document, Node};
use std::path::Path;
use std::str::FromStr;

/// An XML file kept in memory; parse it with [`XmlFile::doc`].
pub struct XmlFile {
    pub path: std::path::PathBuf,
    pub text: String,
}

impl XmlFile {
    pub fn read(path: &Path) -> Result<Self> {
        Ok(XmlFile {
            path: path.to_owned(),
            text: read_text(path)?,
        })
    }

    pub fn doc(&self) -> Result<Document<'_>> {
        parse(&self.path, &self.text)
    }
}

pub fn parse<'a>(path: &Path, text: &'a str) -> Result<Document<'a>> {
    let opts = roxmltree::ParsingOptions {
        allow_dtd: true,
        ..Default::default()
    };
    Document::parse_with_options(text, opts).map_err(|source| Error::Xml {
        path: path.to_owned(),
        source,
    })
}

/// First direct child element called `name`.
pub fn child<'a, 'i>(node: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    node.children()
        .find(|c| c.is_element() && c.tag_name().name() == name)
}

/// All direct child elements called `name`.
pub fn children<'a, 'i: 'a>(
    node: Node<'a, 'i>,
    name: &'a str,
) -> impl Iterator<Item = Node<'a, 'i>> + 'a {
    node.children()
        .filter(move |c| c.is_element() && c.tag_name().name() == name)
}

/// All descendant elements called `name` (document order, like the DOM's
/// `GetElementsByTagName`).
pub fn descendants<'a, 'i: 'a>(
    node: Node<'a, 'i>,
    name: &'a str,
) -> impl Iterator<Item = Node<'a, 'i>> + 'a {
    node.descendants()
        .filter(move |c| c.is_element() && c.tag_name().name() == name && *c != node)
}

/// Element children (skipping text and comments).
pub fn elements<'a, 'i: 'a>(node: Node<'a, 'i>) -> impl Iterator<Item = Node<'a, 'i>> + 'a {
    node.children().filter(|c| c.is_element())
}

/// Concatenated text of all descendant text nodes (DOM `InnerText`).
pub fn inner_text(node: Node) -> String {
    let mut s = String::new();
    for d in node.descendants() {
        if d.is_text()
            && let Some(t) = d.text()
        {
            s.push_str(t);
        }
    }
    s
}

/// Inner text of the first child called `name`.
pub fn child_text(node: Node, name: &str) -> Option<String> {
    child(node, name).map(inner_text)
}

/// Trimmed inner text of the first child called `name`, parsed.
pub fn child_parse<T: FromStr>(node: Node, name: &str) -> Option<T> {
    child_text(node, name).and_then(|t| t.trim().parse().ok())
}

pub fn child_f32(node: Node, name: &str) -> Option<f32> {
    child_text(node, name).and_then(|t| parse_f32(&t))
}

pub fn child_bool(node: Node, name: &str) -> Option<bool> {
    child_text(node, name).and_then(|t| parse_bool(&t))
}

pub fn attr<'a>(node: Node<'a, '_>, name: &str) -> Option<&'a str> {
    node.attribute(name)
}

pub fn attr_parse<T: FromStr>(node: Node, name: &str) -> Option<T> {
    node.attribute(name).and_then(|t| t.trim().parse().ok())
}

pub fn attr_f32(node: Node, name: &str) -> Option<f32> {
    node.attribute(name).and_then(parse_f32)
}

pub fn attr_bool(node: Node, name: &str) -> Option<bool> {
    node.attribute(name).and_then(parse_bool)
}

/// `True`/`False` (any case, surrounding whitespace ignored).
pub fn parse_bool(s: &str) -> Option<bool> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("true") {
        Some(true)
    } else if s.eq_ignore_ascii_case("false") {
        Some(false)
    } else {
        None
    }
}

/// Invariant-culture float (`.4`, `0.25`, `-3`).
pub fn parse_f32(s: &str) -> Option<f32> {
    s.trim().parse().ok()
}

/// Comma-separated integers; empty text gives an empty list. Entries are
/// trimmed; unparsable entries are an error (the game would throw).
pub fn parse_csv_ints(s: &str) -> std::result::Result<Vec<i32>, String> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(Vec::new());
    }
    s.split(',')
        .map(|p| {
            let p = p.trim();
            p.parse::<i32>()
                .map_err(|_| format!("bad integer {p:?} in CSV"))
        })
        .collect()
}

/// Comma-separated strings, trimmed; empty text gives an empty list.
pub fn parse_csv(s: &str) -> Vec<String> {
    let s = s.trim();
    if s.is_empty() {
        return Vec::new();
    }
    s.split(',').map(|p| p.trim().to_owned()).collect()
}

/// `RRGGBB` (or `RRGGBBAA`) hex colour → RGBA.
pub fn parse_hex_color(s: &str) -> Option<[u8; 4]> {
    let s = s.trim().trim_start_matches('#');
    let byte = |i: usize| u8::from_str_radix(s.get(i..i + 2)?, 16).ok();
    match s.len() {
        6 => Some([byte(0)?, byte(2)?, byte(4)?, 255]),
        8 => Some([byte(0)?, byte(2)?, byte(4)?, byte(6)?]),
        _ => None,
    }
}

pub fn child_color(node: Node, name: &str) -> Option<[u8; 4]> {
    child_text(node, name).and_then(|t| parse_hex_color(&t))
}

/// A child `<name x=".." y=".."/>` as a point.
pub fn child_point(node: Node, name: &str) -> Option<(f32, f32)> {
    let c = child(node, name)?;
    Some((attr_f32(c, "x")?, attr_f32(c, "y")?))
}

/// A boolean grid, `width × height` cells, row-major (`y * width + x`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BitGrid {
    pub width: usize,
    pub height: usize,
    pub cells: Vec<bool>,
}

impl BitGrid {
    pub fn new(width: usize, height: usize) -> Self {
        BitGrid {
            width,
            height,
            cells: vec![false; width * height],
        }
    }

    /// Ogmo "Bitstring": `0`/`1` characters, rows separated by `\n`; any other
    /// character (`\r`, spaces) is ignored. Cells beyond the text are `false`,
    /// extra columns/rows are dropped.
    pub fn from_bitstring(text: &str, width: usize, height: usize) -> Self {
        let mut g = BitGrid::new(width, height);
        let (mut x, mut y) = (0usize, 0usize);
        for ch in text.chars() {
            match ch {
                '0' | '1' => {
                    if x < width {
                        g.cells[y * width + x] = ch == '1';
                    }
                    x += 1;
                }
                '\n' => {
                    x = 0;
                    y += 1;
                }
                _ => {}
            }
            if y >= height {
                break;
            }
        }
        g
    }

    #[inline]
    pub fn get(&self, x: usize, y: usize) -> bool {
        self.cells[y * self.width + x]
    }

    #[inline]
    pub fn set(&mut self, x: usize, y: usize, v: bool) {
        self.cells[y * self.width + x] = v;
    }

    pub fn count(&self) -> usize {
        self.cells.iter().filter(|c| **c).count()
    }

    pub fn to_bitstring(&self) -> String {
        let mut s = String::with_capacity((self.width + 1) * self.height);
        for y in 0..self.height {
            if y > 0 {
                s.push('\n');
            }
            for x in 0..self.width {
                s.push(if self.get(x, y) { '1' } else { '0' });
            }
        }
        s
    }
}

/// A tile-index grid (`-1` = empty), row-major.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TileGrid {
    pub width: usize,
    pub height: usize,
    pub cells: Vec<i32>,
}

impl TileGrid {
    pub fn new(width: usize, height: usize) -> Self {
        TileGrid {
            width,
            height,
            cells: vec![-1; width * height],
        }
    }

    /// Ogmo "TrimmedCSV": one text line per row (split on `\n` only, so an
    /// empty line is an empty row — including the line break right after the
    /// opening tag, which makes the first line row 0), comma separated,
    /// trailing `-1`s and trailing rows omitted. Empty fields are skipped and
    /// unparsable fields leave `-1`, as the game does.
    pub fn from_trimmed_csv(text: &str, width: usize, height: usize) -> Self {
        let mut g = TileGrid::new(width, height);
        for (y, line) in text.split('\n').enumerate().take(height) {
            for (x, field) in line.split(',').filter(|f| !f.is_empty()).enumerate() {
                if x >= width {
                    break;
                }
                if let Ok(v) = field.trim().parse::<i32>() {
                    g.cells[y * width + x] = v;
                }
            }
        }
        g
    }

    #[inline]
    pub fn get(&self, x: usize, y: usize) -> i32 {
        self.cells[y * self.width + x]
    }

    #[inline]
    pub fn set(&mut self, x: usize, y: usize, v: i32) {
        self.cells[y * self.width + x] = v;
    }

    pub fn count_set(&self) -> usize {
        self.cells.iter().filter(|c| **c >= 0).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitstring_rows() {
        let g = BitGrid::from_bitstring("101\r\n010\n", 3, 3);
        assert_eq!(g.to_bitstring(), "101\n010\n000");
    }

    #[test]
    fn trimmed_csv_leading_newline_is_row_zero() {
        let g = TileGrid::from_trimmed_csv("\n-1,5\n\n7", 3, 4);
        assert_eq!(g.get(1, 1), 5);
        assert_eq!(g.get(0, 3), 7);
        assert_eq!(g.count_set(), 2);
    }

    #[test]
    fn csv_and_colors() {
        assert_eq!(parse_csv_ints("1, 2 ,3").unwrap(), vec![1, 2, 3]);
        assert!(parse_csv_ints("  ").unwrap().is_empty());
        assert_eq!(parse_hex_color("3CBCFC"), Some([0x3c, 0xbc, 0xfc, 255]));
        assert_eq!(parse_f32(".4"), Some(0.4));
        assert_eq!(parse_bool("True"), Some(true));
    }
}
