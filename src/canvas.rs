use crate::braille::BrailleBuffer;

/// Point type used for drawing
#[derive(Debug, Clone, Copy)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

/// Canvas provides drawing primitives on top of BrailleBuffer.
/// Supports lines, polylines, filled polygons, and text.
pub struct Canvas {
    pub width: usize,
    pub height: usize,
    pub buffer: BrailleBuffer,
}

impl Canvas {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            buffer: BrailleBuffer::new(width, height),
        }
    }

    pub fn frame(&self, use_braille: bool) -> String {
        self.buffer.frame(use_braille)
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
    }

    pub fn set_background(&mut self, color: u8) {
        self.buffer.set_global_background(color);
    }

    pub fn background(&mut self, x: usize, y: usize, color: u8) {
        self.buffer.set_background(x, y, color);
    }

    pub fn text(&mut self, text: &str, x: i32, y: usize, color: u8, center: bool) {
        self.buffer.write_text(text, x, y, color, center);
    }

    pub fn polyline(&mut self, points: &[Point], color: u8, width: i32) {
        for i in 1..points.len() {
            self.line(points[i - 1], points[i], color, width);
        }
    }

    pub fn line(&mut self, from: Point, to: Point, color: u8, width: i32) {
        self.draw_line(from.x, from.y, to.x, to.y, width, color);
    }

    pub fn polygon(&mut self, rings: &[Vec<Point>], color: u8) -> bool {
        let mut vertices: Vec<f64> = Vec::new();
        let mut holes: Vec<usize> = Vec::new();

        for ring in rings {
            if !vertices.is_empty() {
                if ring.len() < 3 {
                    continue;
                }
                holes.push(vertices.len() / 2);
            } else if ring.len() < 3 {
                return false;
            }
            for point in ring {
                vertices.push(point.x as f64);
                vertices.push(point.y as f64);
            }
        }

        let triangles = match earcutr::earcut(&vertices, &holes, 2) {
            Ok(t) => t,
            Err(_) => return false,
        };

        let mut i = 0;
        while i + 2 < triangles.len() {
            let ia = triangles[i];
            let ib = triangles[i + 1];
            let ic = triangles[i + 2];
            let pa = (vertices[ia * 2], vertices[ia * 2 + 1]);
            let pb = (vertices[ib * 2], vertices[ib * 2 + 1]);
            let pc = (vertices[ic * 2], vertices[ic * 2 + 1]);
            self.filled_triangle(pa, pb, pc, color);
            i += 3;
        }
        true
    }

    fn draw_line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, width: i32, color: u8) {
        let w = (width - 1).max(0);
        if w == 0 {
            // Simple bresenham
            self.bresenham_line(x0, y0, x1, y1, color);
            return;
        }

        // Thick line using Zingl's algorithm
        let dx = (x1 - x0).abs();
        let sx: i32 = if x0 < x1 { 1 } else { -1 };
        let dy = (y1 - y0).abs();
        let sy: i32 = if y0 < y1 { 1 } else { -1 };
        let mut err = dx - dy;
        let ed = if dx + dy == 0 {
            1.0
        } else {
            ((dx * dx + dy * dy) as f64).sqrt()
        };
        let wd = (w as f64 + 1.0) / 2.0;

        let mut cx = x0;
        let mut cy = y0;

        loop {
            self.buffer.set_pixel(cx, cy, color);
            let mut e2 = err;
            let mut x2 = cx;
            if 2 * e2 >= -dx {
                e2 += dy;
                let mut y2 = cy;
                while (e2 as f64) < ed * wd && (y1 != y2 || dx > dy) {
                    y2 += sy;
                    self.buffer.set_pixel(cx, y2, color);
                    e2 += dx;
                }
                if cx == x1 {
                    break;
                }
                e2 = err;
                err -= dy;
                cx += sx;
            }
            if 2 * e2 <= dy {
                e2 = dx - e2;
                while (e2 as f64) < ed * wd && (x1 != x2 || dx < dy) {
                    x2 += sx;
                    self.buffer.set_pixel(x2, cy, color);
                    e2 += dy;
                }
                if cy == y1 {
                    break;
                }
                err += dx;
                cy += sy;
            }
        }
    }

    fn bresenham_line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: u8) {
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx: i32 = if x0 < x1 { 1 } else { -1 };
        let sy: i32 = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        let mut cx = x0;
        let mut cy = y0;

        loop {
            self.buffer.set_pixel(cx, cy, color);
            if cx == x1 && cy == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                cx += sx;
            }
            if e2 <= dx {
                err += dx;
                cy += sy;
            }
        }
    }

    fn bresenham_points(&self, x0: i32, y0: i32, x1: i32, y1: i32) -> Vec<Point> {
        let mut points = Vec::new();
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx: i32 = if x0 < x1 { 1 } else { -1 };
        let sy: i32 = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        let mut cx = x0;
        let mut cy = y0;

        loop {
            points.push(Point { x: cx, y: cy });
            if cx == x1 && cy == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                cx += sx;
            }
            if e2 <= dx {
                err += dx;
                cy += sy;
            }
        }
        points
    }

    fn filled_triangle(&mut self, pa: (f64, f64), pb: (f64, f64), pc: (f64, f64), color: u8) {
        let a = self.bresenham_points(pb.0 as i32, pb.1 as i32, pc.0 as i32, pc.1 as i32);
        let b = self.bresenham_points(pa.0 as i32, pa.1 as i32, pc.0 as i32, pc.1 as i32);
        let c = self.bresenham_points(pa.0 as i32, pa.1 as i32, pb.0 as i32, pb.1 as i32);

        let mut points: Vec<Point> = a
            .into_iter()
            .chain(b)
            .chain(c)
            .filter(|p| p.y >= 0 && (p.y as usize) < self.height)
            .collect();

        if points.is_empty() {
            return;
        }

        points.sort_by(|a, b| a.y.cmp(&b.y).then(a.x.cmp(&b.x)));

        // For each row, find min/max X and fill the entire span
        let mut i = 0;
        while i < points.len() {
            let y = points[i].y;
            let mut min_x = points[i].x;
            let mut max_x = points[i].x;

            // Scan all points on this row
            while i < points.len() && points[i].y == y {
                min_x = min_x.min(points[i].x);
                max_x = max_x.max(points[i].x);
                i += 1;
            }

            let left = min_x.max(0);
            let right = max_x.min(self.width as i32 - 1);
            if left <= right {
                for x in left..=right {
                    self.buffer.set_pixel(x, y, color);
                }
            }
        }
    }
}
