use crate::canvas::{Canvas, Point};
use crate::config::MapConfig;
use crate::label::LabelBuffer;
use crate::marker::{self, MapMarker, MarkerAnimation, MarkerShape};
use crate::tile::{ParsedTile, TileFeature};
use crate::tile_source::TileSource;
use crate::utils;

use std::collections::HashMap;
use std::sync::Arc;

/// Information about a tile being rendered
struct RenderTile {
    xyz: (i32, i32, u32),
    zoom: f64,
    position: (f64, f64),
    #[allow(dead_code)]
    size: f64,
    data: Option<ParsedTile>,
    layer_data: HashMap<String, LayerData>,
}

struct LayerData {
    scale: f64,
    features: Vec<TileFeature>,
}

/// The map renderer. Transforms vector tile data into braille/ASCII terminal output.
pub struct Renderer {
    pub width: usize,
    pub height: usize,
    canvas: Canvas,
    label_buffer: LabelBuffer,
    tile_source: Arc<TileSource>,
    pub config: MapConfig,
    tile_padding: i32,
    /// World extent on canvas (min_x, min_y, max_x, max_y) for clipping labels
    world_bounds: (i32, i32, i32, i32),
}

impl Renderer {
    pub fn new(tile_source: Arc<TileSource>, config: MapConfig) -> Self {
        Self {
            width: 0,
            height: 0,
            canvas: Canvas::new(4, 4),
            label_buffer: LabelBuffer::new(),
            tile_source,
            config,
            tile_padding: 64,
            world_bounds: (0, 0, 0, 0),
        }
    }

    pub fn set_size(&mut self, width: usize, height: usize) {
        self.width = width;
        self.height = height;
        self.canvas = Canvas::new(width, height);
    }

    /// Draw the map centered at a given lat/lon and zoom level.
    pub async fn draw(&mut self, center_lon: f64, center_lat: f64, zoom: f64) -> anyhow::Result<()> {
        self.label_buffer.clear();
        self.canvas.clear();

        let mut tiles = self.visible_tiles(center_lon, center_lat, zoom);

        // Compute world extent on canvas for label clipping
        let z = utils::base_zoom(zoom, self.config.tile_range);
        let (cx, cy) = utils::ll2tile(center_lon, center_lat, z);
        let tile_size = utils::tilesize_at_zoom(zoom, self.config.project_size, self.config.tile_range);
        let grid_size = 2_i32.pow(z) as f64;
        let world_x0 = (self.width as f64 / 2.0 - cx * tile_size).floor() as i32;
        let world_y0 = (self.height as f64 / 2.0 - cy * tile_size).floor() as i32;
        let world_x1 = (world_x0 as f64 + grid_size * tile_size).ceil() as i32;
        let world_y1 = (world_y0 as f64 + grid_size * tile_size).ceil() as i32;
        self.world_bounds = (world_x0, world_y0, world_x1, world_y1);

        // Fetch tile data
        for tile in &mut tiles {
            if let Ok(data) = self
                .tile_source
                .get_tile(tile.xyz.2, tile.xyz.0, tile.xyz.1)
                .await
            {
                tile.data = Some(data);
            }
        }

        // Extract features for visible area
        for tile in &mut tiles {
            self.get_tile_features(tile, zoom);
        }

        // Render
        self.render_tiles(&tiles);

        Ok(())
    }

    /// Get the rendered frame as a string
    pub fn frame(&self, use_braille: bool) -> String {
        self.canvas.frame(use_braille)
    }

    fn visible_tiles(&self, center_lon: f64, center_lat: f64, zoom: f64) -> Vec<RenderTile> {
        let z = utils::base_zoom(zoom, self.config.tile_range);
        let (cx, cy) = utils::ll2tile(center_lon, center_lat, z);

        let tile_size = utils::tilesize_at_zoom(zoom, self.config.project_size, self.config.tile_range);
        let mut tiles = Vec::new();

        let grid_size = 2_i32.pow(z);

        // Dynamically compute how many tiles we need to cover the canvas
        let half_w = (self.width as f64 / 2.0 / tile_size).ceil() as i32 + 1;
        let half_h = (self.height as f64 / 2.0 / tile_size).ceil() as i32 + 1;
        let cy_floor = cy.floor() as i32;
        let cx_floor = cx.floor() as i32;

        for ty in (cy_floor - half_h)..=(cy_floor + half_h) {
            for tx in (cx_floor - half_w)..=(cx_floor + half_w) {
                let pos_x = self.width as f64 / 2.0 - (cx - tx as f64) * tile_size;
                let pos_y = self.height as f64 / 2.0 - (cy - ty as f64) * tile_size;

                if tx < 0
                    || tx >= grid_size
                    || ty < 0
                    || ty >= grid_size
                    || pos_x + tile_size < 0.0
                    || pos_y + tile_size < 0.0
                    || pos_x > self.width as f64
                    || pos_y > self.height as f64
                {
                    continue;
                }

                tiles.push(RenderTile {
                    xyz: (tx, ty, z),
                    zoom,
                    position: (pos_x, pos_y),
                    size: tile_size,
                    data: None,
                    layer_data: HashMap::new(),
                });
            }
        }
        tiles
    }

    fn get_tile_features(&self, tile: &mut RenderTile, zoom: f64) {
        let data = match &tile.data {
            Some(d) => d,
            None => return,
        };

        let draw_order = Self::generate_draw_order(zoom);

        for layer_id in &draw_order {
            let layer = match data.layers.get(*layer_id) {
                Some(l) => l,
                None => continue,
            };

            let tile_size = utils::tilesize_at_zoom(zoom, self.config.project_size, self.config.tile_range);
            let scale = layer.extent as f64 / tile_size;

            // Filter features to visible area using bounding box
            let min_x = (-tile.position.0 * scale) as i32;
            let min_y = (-tile.position.1 * scale) as i32;
            let max_x = ((self.width as f64 - tile.position.0) * scale) as i32;
            let max_y = ((self.height as f64 - tile.position.1) * scale) as i32;

            let features: Vec<TileFeature> = layer
                .features
                .iter()
                .filter(|f| {
                    f.max_x >= min_x && f.min_x <= max_x && f.max_y >= min_y && f.min_y <= max_y
                })
                .cloned()
                .collect();

            if !features.is_empty() {
                tile.layer_data.insert(
                    layer_id.to_string(),
                    LayerData { scale, features },
                );
            }
        }
    }

    fn render_tiles(&mut self, tiles: &[RenderTile]) {
        if tiles.is_empty() {
            return;
        }

        let zoom = tiles[0].zoom;
        let draw_order = Self::generate_draw_order(zoom);
        let mut labels = Vec::new();

        for layer_id in &draw_order {
            for tile in tiles {
                let layer = match tile.layer_data.get(*layer_id) {
                    Some(l) => l,
                    None => continue,
                };

                for feature in &layer.features {
                    if layer_id.contains("label") || layer_id.contains("symbol") {
                        labels.push((tile, feature, layer.scale));
                    } else {
                        self.draw_feature(tile, feature, layer.scale);
                    }
                }
            }
        }

        // Sort labels by sort rank
        labels.sort_by(|a, b| a.1.sort.cmp(&b.1.sort));

        if self.config.show_labels {
            for (tile, feature, scale) in labels {
                self.draw_feature(tile, feature, scale);
            }
        }
    }

    fn draw_feature(&mut self, tile: &RenderTile, feature: &TileFeature, scale: f64) {
        if let Some(min_z) = feature.min_zoom {
            if tile.zoom < min_z {
                return;
            }
        }
        if let Some(max_z) = feature.max_zoom {
            if tile.zoom > max_z {
                return;
            }
        }

        match feature.style_type.as_str() {
            "line" => {
                let points: Vec<Point> = self.scale_and_reduce(tile, &feature.points[0], scale, true);
                if points.len() >= 2 {
                    self.canvas
                        .polyline(&points, feature.color, feature.line_width as i32);
                }
            }
            "fill" => {
                let rings: Vec<Vec<Point>> = feature
                    .points
                    .iter()
                    .map(|ring| self.scale_and_reduce(tile, ring, scale, false))
                    .collect();
                self.canvas.polygon(&rings, feature.color);
            }
            "symbol" => {
                let marker = self.config.poi_marker.to_string();
                let text = feature.label.as_deref().unwrap_or(&marker);

                if !feature.points.is_empty() {
                    let scaled = self.scale_and_reduce(tile, &feature.points[0], scale, true);
                    for point in &scaled {
                        // Skip labels outside the world extent (prevents labels in black border)
                        let (wx0, wy0, wx1, wy1) = self.world_bounds;
                        if point.x < wx0 || point.x >= wx1
                            || point.y < wy0 || point.y >= wy1
                            || point.x < 0 || point.x >= self.width as i32
                            || point.y >= self.height as i32
                        {
                            continue;
                        }
                        let x = point.x - text.len() as i32;
                        let margin = self.config.label_margin;
                        if self
                            .label_buffer
                            .write_if_possible(text, x, point.y as usize, Some(margin))
                        {
                            self.canvas.text(text, x, point.y as usize, feature.color, false);
                            break;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn scale_and_reduce(
        &self,
        tile: &RenderTile,
        points: &[crate::tile::GeomPoint],
        scale: f64,
        filter: bool,
    ) -> Vec<Point> {
        let mut scaled = Vec::new();
        let mut last_x = i32::MIN;
        let mut last_y = i32::MIN;
        let mut outside = false;

        let pad = self.tile_padding;
        let min_x = -pad;
        let min_y = -pad;
        let max_x = self.width as i32 + pad;
        let max_y = self.height as i32 + pad;

        for point in points {
            let x = (tile.position.0 + (point.x as f64 / scale)).floor() as i32;
            let y = (tile.position.1 + (point.y as f64 / scale)).floor() as i32;

            if last_x == x && last_y == y {
                continue;
            }
            last_x = x;
            last_y = y;

            if filter {
                if x < min_x || x > max_x || y < min_y || y > max_y {
                    if outside {
                        continue;
                    }
                    outside = true;
                } else {
                    if outside {
                        outside = false;
                        scaled.push(Point { x: last_x, y: last_y });
                    }
                }
            }
            scaled.push(Point { x, y });
        }

        if scaled.len() < 2 {
            return scaled;
        }

        if self.config.simplify_polylines {
            let pts: Vec<(f64, f64)> = scaled.iter().map(|p| (p.x as f64, p.y as f64)).collect();
            let simplified = utils::simplify_points(&pts, 0.5);
            simplified
                .iter()
                .map(|p| Point {
                    x: p.0 as i32,
                    y: p.1 as i32,
                })
                .collect()
        } else {
            scaled
        }
    }

    /// Render markers on top of the map. Call after `draw()` has populated the canvas.
    pub fn draw_markers(
        &mut self,
        markers: &[MapMarker],
        center_lon: f64,
        center_lat: f64,
        zoom: f64,
        tick: u64,
    ) {
        let z = utils::base_zoom(zoom, self.config.tile_range);
        let (cx, cy) = utils::ll2tile(center_lon, center_lat, z);
        let tile_size = utils::tilesize_at_zoom(zoom, self.config.project_size, self.config.tile_range);

        for m in markers {
            if !marker::marker_visible(m.animation, tick) {
                continue;
            }

            // Project lat/lon to pixel coords on canvas
            let (mx, my) = utils::ll2tile(m.lon, m.lat, z);
            let px = (self.width as f64 / 2.0 + (mx - cx) * tile_size).round() as i32;
            let py = (self.height as f64 / 2.0 + (my - cy) * tile_size).round() as i32;

            // Skip off-screen markers (with some padding for labels)
            if px < -20 || px > self.width as i32 + 20 || py < -20 || py > self.height as i32 + 20 {
                continue;
            }

            let color = m.color;

            match m.shape {
                MarkerShape::Dot => {
                    // 3x3 filled dot
                    for dy in -1..=1 {
                        for dx in -1..=1 {
                            self.canvas.buffer.set_pixel(px + dx, py + dy, color);
                        }
                    }
                }
                MarkerShape::Cross => {
                    for d in -3..=3 {
                        self.canvas.buffer.set_pixel(px + d, py, color);
                        self.canvas.buffer.set_pixel(px, py + d, color);
                    }
                }
                MarkerShape::Diamond => {
                    for d in 0..=3 {
                        self.canvas.buffer.set_pixel(px + d, py - 3 + d, color);
                        self.canvas.buffer.set_pixel(px - d, py - 3 + d, color);
                        self.canvas.buffer.set_pixel(px + d, py + 3 - d, color);
                        self.canvas.buffer.set_pixel(px - d, py + 3 - d, color);
                    }
                }
                MarkerShape::Ring(r) => {
                    let radius = if m.animation == MarkerAnimation::Pulse {
                        marker::pulse_radius(tick) as i32
                    } else {
                        r as i32
                    };
                    self.draw_circle(px, py, radius, color);
                }
                MarkerShape::FilledCircle(r) => {
                    let radius = r as i32;
                    for dy in -radius..=radius {
                        for dx in -radius..=radius {
                            if dx * dx + dy * dy <= radius * radius {
                                self.canvas.buffer.set_pixel(px + dx, py + dy, color);
                            }
                        }
                    }
                }
                MarkerShape::Char(ch) => {
                    let text = ch.to_string();
                    self.canvas.text(&text, px, py as usize, color, false);
                }
            }

            // Draw label if present (coordinates are in pixels, matching canvas.text API)
            if let Some(ref label) = m.label {
                let label_x = px + 4; // offset 2 cells (4 pixels) to the right
                let label_y = py as usize;
                let margin = self.config.label_margin;
                if self.label_buffer.write_if_possible(label, label_x, label_y, Some(margin)) {
                    self.canvas.text(label, label_x, label_y, color, false);
                }
            }
        }
    }

    fn draw_circle(&mut self, cx: i32, cy: i32, r: i32, color: u8) {
        // Midpoint circle algorithm
        let mut x = r;
        let mut y = 0;
        let mut d = 1 - r;

        while x >= y {
            self.canvas.buffer.set_pixel(cx + x, cy + y, color);
            self.canvas.buffer.set_pixel(cx - x, cy + y, color);
            self.canvas.buffer.set_pixel(cx + x, cy - y, color);
            self.canvas.buffer.set_pixel(cx - x, cy - y, color);
            self.canvas.buffer.set_pixel(cx + y, cy + x, color);
            self.canvas.buffer.set_pixel(cx - y, cy + x, color);
            self.canvas.buffer.set_pixel(cx + y, cy - x, color);
            self.canvas.buffer.set_pixel(cx - y, cy - x, color);
            y += 1;
            if d <= 0 {
                d += 2 * y + 1;
            } else {
                x -= 1;
                d += 2 * (y - x) + 1;
            }
        }
    }

    fn generate_draw_order(zoom: f64) -> Vec<&'static str> {
        if zoom < 2.0 {
            vec!["water", "landuse", "admin", "country_label", "marine_label"]
        } else {
            vec![
                "landuse",
                "water",
                "marine_label",
                "building",
                "road",
                "admin",
                "country_label",
                "state_label",
                "water_label",
                "place_label",
                "rail_station_label",
                "poi_label",
                "road_label",
                "housenum_label",
            ]
        }
    }
}
