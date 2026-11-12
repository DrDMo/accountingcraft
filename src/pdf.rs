//! PDFs drawn directly in Rust, with no browser involved, so Print, Save PDF and email attachments
//! work the same on every OS. A document is a list of blocks: `layout` flows them onto US Letter
//! pages and `render` writes the file with Open Sans embedded (subset), so it looks the same everywhere.

use krilla::action::LinkAction;
use krilla::annotation::{Annotation, LinkAnnotation, Target};
use krilla::color::rgb;
use krilla::geom::{PathBuilder, Point, Rect, Size, Transform};
use krilla::image::Image;
use krilla::metadata::Metadata;
use krilla::num::NormalizedF32;
use krilla::page::PageSettings;
use krilla::paint::{Fill, FillRule};
use krilla::text::{Font, TextDirection};
use krilla::Document;

const W: f32 = 612.0; // US Letter, in points
const H: f32 = 792.0;
const M: f32 = 54.0; // margin
const CW: f32 = W - 2.0 * M;
const PAD: f32 = 5.0; // table cell padding
// INTER: Inter Regular and SemiBold from craft-fonts when the build had them (see build.rs).
include!(concat!(env!("OUT_DIR"), "/pdf_fonts.rs"));
const REGULAR: &[u8] = match INTER {
    Some((r, _)) => r,
    None => epaint_default_fonts::UBUNTU_LIGHT,
};
const BOLD: &[u8] = match INTER {
    Some((_, b)) => b,
    None => REGULAR,
};

pub type Rgb = (u8, u8, u8);
pub const INK: Rgb = (34, 34, 34);
pub const GRAY: Rgb = (102, 102, 102);
pub const GREEN: Rgb = (26, 127, 55);
const RULE: Rgb = (221, 221, 221);
const TEAL: Rgb = (17, 122, 122);
const WHITE: Rgb = (255, 255, 255);

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Align {
    Left,
    Right,
}

/// How a table row looks: plain with a light rule under it, a muted subtotal line, a bold total
/// with a heavy rule above it, or plain-but-bold (report totals).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Style {
    Normal,
    Sub,
    Total,
    Strong,
}

#[derive(Clone, Debug)]
pub struct Row {
    pub cells: Vec<String>,
    pub style: Style,
}

#[derive(Clone, Debug)]
pub enum Block {
    /// Logo on the left, business name and address on the right.
    Letterhead { logo: Option<Vec<u8>>, name: String, address: String },
    Title(String),
    Heading(String),
    /// Wrapped text; `\n` starts a new line.
    Text { text: String, size: f32, bold: bool, color: Rgb },
    Bullet(String),
    /// Columns are (header, alignment); widths follow the content.
    Table { cols: Vec<(String, Align)>, rows: Vec<Row> },
    /// A filled button that opens `url`.
    Button { text: String, url: String },
    Space(f32),
    PageBreak,
}

impl Block {
    pub fn text(s: impl Into<String>) -> Block {
        Block::Text { text: s.into(), size: 11.0, bold: false, color: INK }
    }
    pub fn bold(s: impl Into<String>) -> Block {
        Block::Text { text: s.into(), size: 11.0, bold: true, color: INK }
    }
    pub fn note(s: impl Into<String>) -> Block {
        Block::Text { text: s.into(), size: 10.0, bold: false, color: GRAY }
    }
}

/// Every piece of text in the blocks, in order, for tests and search.
#[cfg(test)]
pub fn plain_text(blocks: &[Block]) -> String {
    let mut out = String::new();
    for b in blocks {
        match b {
            Block::Letterhead { name, address, .. } => out += &format!("{name}\n{address}\n"),
            Block::Title(t) | Block::Heading(t) | Block::Bullet(t) | Block::Text { text: t, .. } | Block::Button { text: t, .. } => out += &format!("{t}\n"),
            Block::Table { cols, rows } => {
                out += &(cols.iter().map(|c| c.0.as_str()).collect::<Vec<_>>().join(" | ") + "\n");
                for r in rows {
                    out += &(r.cells.join(" | ") + "\n");
                }
            }
            Block::Space(_) | Block::PageBreak => {}
        }
    }
    out
}

/// One drawing step on a page. `y` is the text baseline or the top of a box.
enum Op {
    Text { x: f32, y: f32, size: f32, bold: bool, color: Rgb, s: String },
    Rect { x: f32, y: f32, w: f32, h: f32, color: Rgb },
    Image { x: f32, y: f32, w: f32, h: f32, img: Image },
    Link { x: f32, y: f32, w: f32, h: f32, url: String },
}

struct Layout {
    pages: Vec<Vec<Op>>,
    y: f32,
    reg: ttf_parser::Face<'static>,
    bold: ttf_parser::Face<'static>,
}

impl Layout {
    fn face(&self, bold: bool) -> &ttf_parser::Face<'static> {
        if bold { &self.bold } else { &self.reg }
    }

    fn width(&self, s: &str, size: f32, bold: bool) -> f32 {
        let f = self.face(bold);
        let units: f32 = s.chars().map(|c| f.glyph_index(c).and_then(|g| f.glyph_hor_advance(g)).unwrap_or(f.units_per_em() / 2) as f32).sum();
        units * size / f.units_per_em() as f32
    }

    fn line_height(size: f32) -> f32 {
        size * 1.45
    }

    /// Splits text into lines no wider than `width`; very long words are broken by character.
    fn wrap(&self, text: &str, size: f32, bold: bool, width: f32) -> Vec<String> {
        let mut out = vec![];
        for para in text.split('\n') {
            let mut line = String::new();
            for (i, word) in para.split(' ').enumerate() {
                let cand = if i == 0 { word.to_string() } else { format!("{line} {word}") };
                if i == 0 || self.width(&cand, size, bold) <= width {
                    line = cand;
                } else {
                    out.push(std::mem::take(&mut line));
                    line = word.to_string();
                }
                while self.width(&line, size, bold) > width && line.chars().count() > 1 {
                    let mut head = String::new();
                    for c in line.chars() {
                        if !head.is_empty() && self.width(&format!("{head}{c}"), size, bold) > width {
                            break;
                        }
                        head.push(c);
                    }
                    line = line[head.len()..].to_string();
                    out.push(head);
                }
            }
            out.push(line);
        }
        out
    }

    fn push(&mut self, op: Op) {
        if let Some(p) = self.pages.last_mut() {
            p.push(op);
        }
    }

    fn new_page(&mut self) {
        self.pages.push(vec![]);
        self.y = M;
    }

    /// Starts a new page unless `h` more points fit on this one.
    fn room(&mut self, h: f32) -> bool {
        let fits = self.y + h <= H - M || self.y <= M;
        if !fits {
            self.new_page();
        }
        !fits
    }

    /// Draws one line of text whose line box starts at `top`.
    fn text(&mut self, x: f32, top: f32, s: &str, size: f32, bold: bool, color: Rgb) {
        let f = self.face(bold);
        let upm = f.units_per_em() as f32;
        let (asc, desc) = (f.ascender() as f32 * size / upm, f.descender() as f32 * size / upm);
        let baseline = top + (Self::line_height(size) - (asc - desc)) / 2.0 + asc;
        self.push(Op::Text { x, y: baseline, size, bold, color, s: s.to_string() });
    }

    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Rgb) {
        self.push(Op::Rect { x, y, w, h, color });
    }

    fn paragraph(&mut self, x: f32, width: f32, text: &str, size: f32, bold: bool, color: Rgb) {
        let lh = Self::line_height(size);
        for line in self.wrap(text, size, bold, width) {
            self.room(lh);
            let y = self.y;
            self.text(x, y, &line, size, bold, color);
            self.y += lh;
        }
    }

    /// Column widths: right-aligned (money) columns get their natural width; left-aligned (text)
    /// columns share the rest in proportion to their longest line, wrapping when they must.
    fn widths(&self, cols: &[(String, Align)], rows: &[Row], size: f32) -> Vec<f32> {
        let natural: Vec<f32> = (0..cols.len())
            .map(|i| {
                let cells = rows.iter().filter(|r| !spans(r)).filter_map(|r| r.cells.get(i)).chain(std::iter::once(&cols[i].0));
                cells.flat_map(|c| c.split('\n')).map(|l| self.width(l, size, true)).fold(0.0, f32::max) + 2.0 * PAD + 1.0
            })
            .collect();
        let flex: f32 = natural.iter().zip(cols).filter(|(_, c)| c.1 == Align::Left).map(|(n, _)| n).sum();
        let fixed: f32 = natural.iter().sum::<f32>() - flex;
        if flex <= 0.0 || fixed >= CW {
            let total: f32 = natural.iter().sum::<f32>().max(1.0);
            return natural.iter().map(|n| CW * n / total).collect();
        }
        let avail = CW - fixed;
        if flex <= avail {
            let scale = avail / flex; // room to spare: widen the text columns evenly
            return natural.iter().zip(cols).map(|(n, c)| if c.1 == Align::Left { n * scale } else { *n }).collect();
        }
        // Too wide: short text columns (dates, numbers) keep their width and the long ones share
        // what's left, so only the long ones wrap.
        let mut left: Vec<usize> = (0..cols.len()).filter(|&i| cols[i].1 == Align::Left).collect();
        left.sort_by(|&a, &b| natural[a].total_cmp(&natural[b]));
        let mut out = natural.clone();
        let mut room = avail;
        for (k, &i) in left.iter().enumerate() {
            let fair = room / (left.len() - k) as f32;
            out[i] = natural[i].min(fair);
            room -= out[i];
        }
        out
    }

    fn table(&mut self, cols: &[(String, Align)], rows: &[Row]) {
        let size = 10.0;
        let lh = Self::line_height(size);
        let mut xs = vec![M];
        for w in self.widths(cols, rows, size) {
            let last = xs.last().copied().unwrap_or(M);
            xs.push(last + w);
        }
        let header = Row { cells: cols.iter().map(|c| c.0.clone()).collect(), style: Style::Strong };
        // Each cell's wrapped lines; a row with only its first cell filled (a section name) spans the table.
        let lines = |l: &Layout, row: &Row, bold: bool| -> Vec<Vec<String>> {
            let span = spans(row);
            (0..cols.len())
                .map(|i| {
                    let w = if span && i == 0 { CW } else { xs[i + 1] - xs[i] };
                    l.wrap(row.cells.get(i).map_or("", |s| s.as_str()), size, bold, w - 2.0 * PAD)
                })
                .collect()
        };
        let draw = |l: &mut Layout, row: &Row, head: bool| {
            let bold = head || matches!(row.style, Style::Total | Style::Strong);
            let color = if row.style == Style::Sub { GRAY } else { INK };
            let cells = lines(l, row, bold);
            let h = cells.iter().map(Vec::len).max().unwrap_or(1) as f32 * lh + 2.0 * PAD;
            let top = l.y;
            if row.style == Style::Total {
                l.rect(M, top, CW, 1.5, INK);
            }
            for (i, lines) in cells.iter().enumerate() {
                for (n, line) in lines.iter().enumerate() {
                    let x = match cols[i].1 {
                        Align::Left => xs[i] + PAD,
                        Align::Right => xs[i + 1] - PAD - l.width(line, size, bold),
                    };
                    l.text(x, top + PAD + n as f32 * lh, line, size, bold, color);
                }
            }
            match (head, row.style) {
                (true, _) => l.rect(M, top + h - 1.2, CW, 1.2, INK),
                (false, Style::Normal | Style::Strong) => l.rect(M, top + h - 0.6, CW, 0.6, RULE),
                _ => {}
            }
            l.y += h;
        };
        self.room(2.0 * (lh + 2.0 * PAD));
        draw(self, &header, true);
        for r in rows {
            let n = lines(self, r, true).iter().map(Vec::len).max().unwrap_or(1);
            let keep = if spans(r) { lh + 2.0 * PAD } else { 0.0 }; // a section name stays with its first line
            if self.room(n as f32 * lh + 2.0 * PAD + keep) {
                draw(self, &header, true); // repeat the header on each new page
            }
            draw(self, r, false);
        }
        self.y += 10.0;
    }
}

/// A row with text only in its first cell, like an account name heading a group of lines.
fn spans(row: &Row) -> bool {
    row.cells.len() > 1 && row.cells.iter().skip(1).all(|c| c.is_empty())
}

/// The block with its ISO dates written for people ("Nov 4, 2026").
fn friendly_dates(b: &Block) -> Block {
    let n = crate::books::nice_dates;
    match b.clone() {
        Block::Title(s) => Block::Title(n(&s)),
        Block::Heading(s) => Block::Heading(n(&s)),
        Block::Text { text, size, bold, color } => Block::Text { text: n(&text), size, bold, color },
        Block::Bullet(s) => Block::Bullet(n(&s)),
        Block::Table { cols, rows } => Block::Table { cols, rows: rows.into_iter().map(|r| Row { cells: r.cells.iter().map(|c| n(c)).collect(), ..r }).collect() },
        other => other,
    }
}

fn layout(blocks: &[Block]) -> Result<Vec<Vec<Op>>, String> {
    let face = |b: &'static [u8]| ttf_parser::Face::parse(b, 0).map_err(|e| format!("bundled font is unreadable: {e}"));
    let mut l = Layout { pages: vec![vec![]], y: M, reg: face(REGULAR)?, bold: face(BOLD)? };
    let blocks: Vec<Block> = blocks.iter().map(friendly_dates).collect();
    for b in &blocks {
        match b {
            Block::Letterhead { logo, name, address } => {
                let mut h: f32 = 0.0;
                if let Some(img) = logo.as_ref().and_then(|d| logo_image(d)) {
                    let (iw, ih) = img.size();
                    let scale = (200.0 / iw.max(1) as f32).min(80.0 / ih.max(1) as f32);
                    let (w, ih) = (iw as f32 * scale, ih as f32 * scale);
                    let y = l.y;
                    l.push(Op::Image { x: M, y, w, h: ih, img });
                    h = ih;
                }
                let mut y = l.y;
                for (i, line) in std::iter::once(name.as_str()).chain(address.lines()).filter(|s| !s.trim().is_empty()).enumerate() {
                    let (size, bold) = if i == 0 { (12.0, true) } else { (10.0, false) };
                    let x = W - M - l.width(line, size, bold);
                    l.text(x, y, line, size, bold, INK);
                    y += Layout::line_height(size);
                }
                l.y += h.max(y - l.y) + 28.0;
            }
            Block::Title(t) => {
                l.paragraph(M, CW, t, 21.0, true, INK);
                l.y += 4.0;
            }
            Block::Heading(t) => {
                l.y += 10.0;
                l.room(3.0 * Layout::line_height(14.0)); // keep a heading with what follows it
                l.paragraph(M, CW, t, 14.0, true, TEAL);
                l.y += 2.0;
            }
            Block::Text { text, size, bold, color } => {
                l.paragraph(M, CW, text, *size, *bold, *color);
                l.y += 6.0;
            }
            Block::Bullet(t) => {
                let lh = Layout::line_height(11.0);
                l.room(lh);
                let y = l.y;
                l.text(M + 6.0, y, "•", 11.0, false, INK);
                l.paragraph(M + 20.0, CW - 20.0, t, 11.0, false, INK);
                l.y += 2.0;
            }
            Block::Table { cols, rows } => l.table(cols, rows),
            Block::Button { text, url } => {
                let (w, h) = (l.width(text, 11.0, true) + 36.0, 30.0);
                l.y += 12.0;
                l.room(h);
                let y = l.y;
                l.rect(M, y, w, h, TEAL);
                l.text(M + 18.0, y + (h - Layout::line_height(11.0)) / 2.0, text, 11.0, true, WHITE);
                l.push(Op::Link { x: M, y, w, h, url: url.clone() });
                l.y += h + 8.0;
            }
            Block::Space(s) => l.y += s,
            Block::PageBreak => {
                if l.pages.last().is_some_and(|p| !p.is_empty()) {
                    l.new_page();
                }
            }
        }
    }
    Ok(l.pages)
}

/// The logo as a PDF image; None (no logo shown) when it isn't a readable PNG or JPEG.
fn logo_image(data: &[u8]) -> Option<Image> {
    let d = krilla::Data::from(data.to_vec());
    if data.starts_with(b"\x89PNG") { Image::from_png(d, true).ok() } else { Image::from_jpeg(d, true).ok() }
}

fn fill(c: Rgb) -> Fill {
    Fill { paint: rgb::Color::new(c.0, c.1, c.2).into(), opacity: NormalizedF32::ONE, rule: FillRule::NonZero }
}

/// Lays out `blocks` and returns the finished PDF file.
pub fn render(title: &str, blocks: &[Block]) -> Result<Vec<u8>, String> {
    let pages = layout(blocks)?;
    let font = |b: &'static [u8]| Font::new(krilla::Data::from(b), 0).ok_or("bundled font is unreadable".to_string());
    let (reg, bold) = (font(REGULAR)?, font(BOLD)?);
    let mut doc = Document::new();
    doc.set_metadata(Metadata::new().title(title.to_string()).creator("AccountingCraft".into()));
    for ops in pages {
        let mut page = doc.start_page_with(PageSettings::from_wh(W, H).ok_or("bad page size")?);
        let mut links = vec![];
        let mut s = page.surface();
        for op in ops {
            match op {
                Op::Text { x, y, size, bold: b, color, s: text } => {
                    s.set_fill(Some(fill(color)));
                    s.draw_text(Point::from_xy(x, y), if b { bold.clone() } else { reg.clone() }, size, &text, false, TextDirection::Auto);
                }
                Op::Rect { x, y, w, h, color } => {
                    if let Some(r) = Rect::from_xywh(x, y, w, h) {
                        let mut pb = PathBuilder::new();
                        pb.push_rect(r);
                        if let Some(path) = pb.finish() {
                            s.set_fill(Some(fill(color)));
                            s.draw_path(&path);
                        }
                    }
                }
                Op::Image { x, y, w, h, img } => {
                    if let Some(size) = Size::from_wh(w, h) {
                        s.push_transform(&Transform::from_translate(x, y));
                        s.draw_image(img, size);
                        s.pop();
                    }
                }
                Op::Link { x, y, w, h, url } => links.push((x, y, w, h, url)),
            }
        }
        s.finish();
        for (x, y, w, h, url) in links {
            if let Some(r) = Rect::from_xywh(x, y, w, h) {
                page.add_annotation(Annotation::new_link(LinkAnnotation::new(r, Target::Action(LinkAction::new(url).into())), None));
            }
        }
        page.finish();
    }
    doc.finish().map_err(|e| format!("couldn't make the PDF: {e:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_documents_flow_onto_more_pages() {
        let rows: Vec<Row> = (0..120).map(|i| Row { cells: vec![format!("Line {i} with a fairly long description that wraps"), "$1.00".into()], style: Style::Normal }).collect();
        let blocks = vec![
            Block::Letterhead { logo: None, name: "Acme".into(), address: "1 Main St\nTown".into() },
            Block::Title("Invoice #1".into()),
            Block::Table { cols: vec![("Description".into(), Align::Left), ("Amount".into(), Align::Right)], rows },
            Block::Button { text: "Pay online".into(), url: "https://example.com".into() },
            Block::PageBreak,
            Block::Heading("Next".into()),
            Block::Bullet("A point".into()),
        ];
        assert!(layout(&blocks).unwrap().len() >= 4, "table spills onto extra pages, then the page break");
        let pdf = render("Test", &blocks).unwrap();
        assert!(pdf.starts_with(b"%PDF"));
    }

    #[test]
    fn wrapping_keeps_indents_and_breaks_long_words() {
        let l = Layout { pages: vec![vec![]], y: M, reg: ttf_parser::Face::parse(REGULAR, 0).unwrap(), bold: ttf_parser::Face::parse(BOLD, 0).unwrap() };
        assert_eq!(l.wrap("    Rent", 10.0, false, 300.0), vec!["    Rent"]);
        let long = l.wrap(&"x".repeat(200), 10.0, false, 100.0);
        assert!(long.len() > 1 && long.iter().all(|s| l.width(s, 10.0, false) <= 100.0));
        assert_eq!(l.wrap("a\nb", 10.0, false, 300.0), vec!["a", "b"]);
    }
}
