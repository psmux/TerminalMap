/// 2D spatial index for label collision detection.
/// Uses a simple grid-based approach analogous to RBush.
pub struct LabelBuffer {
    entries: Vec<LabelRect>,
    margin: f64,
}

#[derive(Debug, Clone)]
struct LabelRect {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
}

impl LabelBuffer {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            margin: 5.0,
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    fn project(x: i32, y: usize) -> (f64, f64) {
        ((x as f64 / 2.0).floor(), (y as f64 / 4.0).floor())
    }

    pub fn write_if_possible(
        &mut self,
        text: &str,
        x: i32,
        y: usize,
        margin: Option<f64>,
    ) -> bool {
        let margin = margin.unwrap_or(self.margin);
        let (px, py) = Self::project(x, y);

        if self.has_space(text, px, py, margin) {
            let area = Self::calculate_area(text, px, py, margin);
            self.entries.push(area);
            true
        } else {
            false
        }
    }

    fn has_space(&self, text: &str, x: f64, y: f64, margin: f64) -> bool {
        let area = Self::calculate_area(text, x, y, margin);
        !self
            .entries
            .iter()
            .any(|e| Self::rects_overlap(&area, e))
    }

    fn rects_overlap(a: &LabelRect, b: &LabelRect) -> bool {
        a.min_x <= b.max_x && a.max_x >= b.min_x && a.min_y <= b.max_y && a.max_y >= b.min_y
    }

    fn calculate_area(text: &str, x: f64, y: f64, margin: f64) -> LabelRect {
        let text_width = text.chars().count() as f64;
        LabelRect {
            min_x: x - margin,
            min_y: y - margin / 2.0,
            max_x: x + margin + text_width,
            max_y: y + margin / 2.0,
        }
    }
}
