use crate::utils;

/// Braille dot mapping: each character cell is 2x4 pixels
/// The braille character base is U+2800
/// Dot positions:
///   0  3
///   1  4
///   2  5
///   6  7
const BRAILLE_MAP: [[u8; 2]; 4] = [
    [0x01, 0x08],
    [0x02, 0x10],
    [0x04, 0x20],
    [0x40, 0x80],
];

/// ASCII block character fallback mapping
struct AsciiMapping {
    mask: u8,
    ch: char,
}

fn ascii_mappings() -> Vec<AsciiMapping> {
    vec![
        AsciiMapping { mask: 1 + 2 + 16 + 32, ch: '\u{2580}' },   // ▀
        AsciiMapping { mask: 4 + 8 + 64 + 128, ch: '\u{2584}' },  // ▄
        AsciiMapping { mask: 2 + 4 + 32 + 64, ch: '\u{25A0}' },   // ■
        AsciiMapping { mask: 1 + 2 + 4 + 8, ch: '\u{258C}' },     // ▌
        AsciiMapping { mask: 16 + 32 + 64 + 128, ch: '\u{2590}' }, // ▐
        AsciiMapping { mask: 255, ch: '\u{2588}' },                // █
    ]
}

/// A buffer that maps pixels to braille unicode characters with color support.
/// Each terminal character cell represents a 2x4 pixel grid.
pub struct BrailleBuffer {
    pub width: usize,
    pub height: usize,
    pixel_buffer: Vec<u8>,
    char_buffer: Vec<Option<char>>,
    foreground_buffer: Vec<u8>,
    background_buffer: Vec<u8>,
    /// Color of each individual pixel; a cell's final color is the majority
    /// color of its lit pixels so a thin line crossing a filled cell cannot
    /// recolor the whole cell (prevents e.g. white borders bleeding into ocean)
    pixel_colors: Vec<u8>,
    /// Cells whose color was set directly (markers) and must not be
    /// overridden by the majority vote
    color_locked: Vec<bool>,
    global_background: Option<u8>,
    ascii_to_braille: Vec<char>,
}

impl BrailleBuffer {
    pub fn new(width: usize, height: usize) -> Self {
        let size = (width / 2) * (height / 4);
        let mut buf = Self {
            width,
            height,
            pixel_buffer: vec![0; size],
            char_buffer: vec![None; size],
            foreground_buffer: vec![0; size],
            background_buffer: vec![0; size],
            pixel_colors: vec![0; width * height],
            color_locked: vec![false; size],
            global_background: None,
            ascii_to_braille: Vec::new(),
        };
        buf.map_braille();
        buf
    }

    pub fn clear(&mut self) {
        self.pixel_buffer.fill(0);
        self.char_buffer.fill(None);
        self.foreground_buffer.fill(0);
        self.background_buffer.fill(0);
        self.pixel_colors.fill(0);
        self.color_locked.fill(false);
    }

    pub fn set_global_background(&mut self, bg: u8) {
        self.global_background = Some(bg);
    }

    pub fn set_background(&mut self, x: usize, y: usize, color: u8) {
        if x < self.width && y < self.height {
            let idx = self.project(x, y);
            if idx < self.background_buffer.len() {
                self.background_buffer[idx] = color;
            }
        }
    }

    pub fn set_pixel(&mut self, x: i32, y: i32, color: u8) {
        if x < 0 || y < 0 {
            return;
        }
        let x = x as usize;
        let y = y as usize;
        if x >= self.width || y >= self.height {
            return;
        }
        let idx = self.project(x, y);
        let mask = BRAILLE_MAP[y & 3][x & 1];
        if idx < self.pixel_buffer.len() {
            self.pixel_buffer[idx] |= mask;
            self.foreground_buffer[idx] = color;
            self.pixel_colors[y * self.width + x] = color;
        }
    }

    /// Set a pixel and force its color onto the whole cell, bypassing the
    /// majority vote. Used for markers which must always be visible on top.
    pub fn set_pixel_forced(&mut self, x: i32, y: i32, color: u8) {
        if x < 0 || y < 0 {
            return;
        }
        let ux = x as usize;
        let uy = y as usize;
        if ux >= self.width || uy >= self.height {
            return;
        }
        let idx = self.project(ux, uy);
        if idx < self.pixel_buffer.len() {
            self.set_pixel(x, y, color);
            self.color_locked[idx] = true;
        }
    }

    /// Resolve the final foreground color of a cell as the majority color of
    /// its lit pixels. Ties are broken by the lit pixel colors of the eight
    /// surrounding cells so area fills (like ocean) win over 1px lines.
    fn resolve_cell_color(&self, cell_x: usize, cell_y: usize) -> u8 {
        let idx = cell_y * (self.width >> 1) + cell_x;
        if self.color_locked[idx] {
            return self.foreground_buffer[idx];
        }
        let mut counts: [(u8, u32); 8] = [(0, 0); 8];
        let mut n = 0usize;
        let px0 = cell_x * 2;
        let py0 = cell_y * 4;
        for dy in 0..4 {
            for dx in 0..2 {
                let (px, py) = (px0 + dx, py0 + dy);
                if px >= self.width || py >= self.height {
                    continue;
                }
                let mask = BRAILLE_MAP[py & 3][px & 1];
                let cell_idx = self.project(px, py);
                if self.pixel_buffer[cell_idx] & mask == 0 {
                    continue;
                }
                let c = self.pixel_colors[py * self.width + px];
                if let Some(slot) = counts[..n].iter_mut().find(|(col, _)| *col == c) {
                    slot.1 += 1;
                } else if n < counts.len() {
                    counts[n] = (c, 1);
                    n += 1;
                }
            }
        }
        if n == 0 {
            return self.foreground_buffer[idx];
        }
        let max_count = counts[..n].iter().map(|(_, cnt)| *cnt).max().unwrap_or(0);
        let tied: Vec<u8> = counts[..n]
            .iter()
            .filter(|(_, cnt)| *cnt == max_count)
            .map(|(col, _)| *col)
            .collect();
        if tied.len() == 1 {
            return tied[0];
        }
        // Tie: count lit pixels of each tied color in neighboring cells
        let cols = self.width >> 1;
        let rows = self.height >> 2;
        let mut best = (tied[0], 0u32);
        for &cand in &tied {
            let mut score = 0u32;
            for ny in cell_y.saturating_sub(1)..=(cell_y + 1).min(rows.saturating_sub(1)) {
                for nx in cell_x.saturating_sub(1)..=(cell_x + 1).min(cols.saturating_sub(1)) {
                    if nx == cell_x && ny == cell_y {
                        continue;
                    }
                    for dy in 0..4 {
                        for dx in 0..2 {
                            let (px, py) = (nx * 2 + dx, ny * 4 + dy);
                            if px >= self.width || py >= self.height {
                                continue;
                            }
                            let mask = BRAILLE_MAP[py & 3][px & 1];
                            if self.pixel_buffer[self.project(px, py)] & mask != 0
                                && self.pixel_colors[py * self.width + px] == cand
                            {
                                score += 1;
                            }
                        }
                    }
                }
            }
            if score > best.1 {
                best = (cand, score);
            }
        }
        best.0
    }

    fn project(&self, x: usize, y: usize) -> usize {
        (x >> 1) + (self.width >> 1) * (y >> 2)
    }

    fn map_braille(&mut self) {
        let mappings = ascii_mappings();
        self.ascii_to_braille = vec![' '; 256];

        for i in 1..=255u16 {
            let mut best_char = ' ';
            let mut best_covered = 0u32;

            for m in &mappings {
                let covered = utils::population((m.mask as u32) & (i as u32));
                if covered > best_covered {
                    best_covered = covered;
                    best_char = m.ch;
                }
            }
            self.ascii_to_braille[i as usize] = best_char;
        }
    }

    fn term_color(&self, foreground: u8, background: u8) -> String {
        let bg = if let Some(gb) = self.global_background {
            background | gb
        } else {
            background
        };

        if foreground != 0 && bg != 0 {
            format!("\x1B[38;5;{};48;5;{}m", foreground, bg)
        } else if foreground != 0 {
            format!("\x1B[49;38;5;{}m", foreground)
        } else if bg != 0 {
            format!("\x1B[39;48;5;{}m", bg)
        } else {
            "\x1B[39;49m".to_string()
        }
    }

    /// Render the buffer to a string of braille/ASCII characters with ANSI color codes
    pub fn frame(&self, use_braille: bool) -> String {
        let cols = self.width / 2;
        let rows = self.height / 4;
        // Pre-allocate: each cell could be ~20 chars for color + 4 for char
        let mut output = String::with_capacity(cols * rows * 8);
        let mut current_color = String::new();

        for y in 0..rows {
            if y > 0 {
                output.push_str("\r\n");
            }
            let mut skip: usize = 0;

            for x in 0..cols {
                let idx = y * cols + x;
                if idx >= self.pixel_buffer.len() {
                    break;
                }

                let fg = if self.char_buffer[idx].is_some() {
                    self.foreground_buffer[idx]
                } else {
                    self.resolve_cell_color(x, y)
                };
                let color_code = self.term_color(fg, self.background_buffer[idx]);
                if current_color != color_code {
                    output.push_str(&color_code);
                    current_color = color_code;
                }

                if let Some(ch) = self.char_buffer[idx] {
                    // Character labels take priority
                    let char_width = unicode_width(ch);
                    if char_width > 1 {
                        skip += char_width - 1;
                    }
                    if skip + x < cols {
                        output.push(ch);
                    }
                } else if skip == 0 {
                    if use_braille {
                        // U+2800 is the braille base character
                        let braille_char =
                            char::from_u32(0x2800 + self.pixel_buffer[idx] as u32).unwrap_or(' ');
                        output.push(braille_char);
                    } else {
                        output.push(self.ascii_to_braille[self.pixel_buffer[idx] as usize]);
                    }
                } else {
                    skip -= 1;
                }
            }
        }

        output.push_str("\x1B[39;49m");
        output
    }

    pub fn set_char(&mut self, ch: char, x: usize, y: usize, color: u8) {
        if x < self.width && y < self.height {
            let idx = self.project(x, y);
            if idx < self.char_buffer.len() {
                self.char_buffer[idx] = Some(ch);
                self.foreground_buffer[idx] = color;
            }
        }
    }

    pub fn write_text(&mut self, text: &str, x: i32, y: usize, color: u8, center: bool) {
        let x = if center {
            x - (text.len() as i32 / 2 + 1)
        } else {
            x
        };
        for (i, ch) in text.chars().enumerate() {
            let px = x + (i as i32) * 2;
            if px >= 0 {
                self.set_char(ch, px as usize, y, color);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WATER: u8 = 69;
    const WHITE: u8 = 231;

    /// A thin line crossing a fully lit cell must not recolor the water dots
    /// (white borders bleeding into the ocean)
    #[test]
    fn line_does_not_bleed_into_filled_cell() {
        let mut buf = BrailleBuffer::new(8, 8);
        // Fill everything with water
        for y in 0..8 {
            for x in 0..8 {
                buf.set_pixel(x, y, WATER);
            }
        }
        // Draw a 1px horizontal white line through the middle cell row
        for x in 0..8 {
            buf.set_pixel(x, 1, WHITE);
        }
        // Every cell still resolves to the water color (6 of 8 dots are water)
        for cy in 0..2 {
            for cx in 0..4 {
                assert_eq!(buf.resolve_cell_color(cx, cy), WATER);
            }
        }
    }

    /// On unlit (land) cells the line color must win
    #[test]
    fn line_visible_on_empty_cells() {
        let mut buf = BrailleBuffer::new(8, 8);
        for x in 0..8 {
            buf.set_pixel(x, 1, WHITE);
        }
        assert_eq!(buf.resolve_cell_color(0, 0), WHITE);
        assert_eq!(buf.resolve_cell_color(3, 0), WHITE);
    }

    /// A vertical line covering half a filled cell ties 4-4; the neighbor
    /// vote must side with the surrounding water fill
    #[test]
    fn tie_resolved_by_neighbors() {
        let mut buf = BrailleBuffer::new(8, 8);
        for y in 0..8 {
            for x in 0..8 {
                buf.set_pixel(x, y, WATER);
            }
        }
        // Vertical white line down one pixel column of cell (1,0)
        for y in 0..4 {
            buf.set_pixel(2, y, WHITE);
        }
        assert_eq!(buf.resolve_cell_color(1, 0), WATER);
    }

    /// Markers use forced pixels and must always win the cell color
    #[test]
    fn forced_pixels_override_majority() {
        let mut buf = BrailleBuffer::new(8, 8);
        for y in 0..8 {
            for x in 0..8 {
                buf.set_pixel(x, y, WATER);
            }
        }
        buf.set_pixel_forced(2, 1, 196);
        assert_eq!(buf.resolve_cell_color(1, 0), 196);
    }
}

/// Simple unicode width approximation
fn unicode_width(c: char) -> usize {
    if c.is_ascii() {
        1
    } else {
        // CJK characters and some symbols are double-width
        let cp = c as u32;
        if (0x1100..=0x115F).contains(&cp)
            || (0x2E80..=0x303E).contains(&cp)
            || (0x3040..=0xA4CF).contains(&cp)
            || (0xAC00..=0xD7A3).contains(&cp)
            || (0xF900..=0xFAFF).contains(&cp)
            || (0xFE30..=0xFE6F).contains(&cp)
            || (0xFF01..=0xFF60).contains(&cp)
            || (0xFFE0..=0xFFE6).contains(&cp)
        {
            2
        } else {
            1
        }
    }
}
