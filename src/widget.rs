use crate::camera::Camera;
use crate::config::MapConfig;
use crate::marker::{MapMarker, MarkerAnimation};
use crate::renderer::Renderer;
use crate::styler::Styler;
use crate::tile_source::TileSource;
use crate::utils;

use std::sync::Arc;
use tokio::sync::Mutex;

/// The `MapState` holds the current map view state and is the primary interface
/// for embedding a map into any TUI application.
///
/// # Usage as a reusable component:
/// ```rust,no_run
/// use terminalmap::widget::MapState;
/// use terminalmap::config::MapConfig;
///
/// #[tokio::main]
/// async fn main() {
///     let config = MapConfig::default();
///     let mut map = MapState::new(config).await.unwrap();
///     map.set_size(120, 60);
///     let frame = map.render().await.unwrap();
///     print!("{}", frame);
/// }
/// ```
pub struct MapState {
    pub center_lat: f64,
    pub center_lon: f64,
    pub zoom: f64,
    pub config: MapConfig,
    pub width: usize,
    pub height: usize,
    pub tick: u64,
    min_zoom: f64,
    markers: Vec<MapMarker>,
    camera: Camera,
    renderer: Arc<Mutex<Renderer>>,
}

impl MapState {
    /// Create a new MapState with the given configuration.
    /// This initializes the tile source, styler, and renderer.
    pub async fn new(config: MapConfig) -> anyhow::Result<Self> {
        let style_json = if let Some(ref data) = config.style_data {
            serde_json::from_str(data)?
        } else {
            // Load the embedded dark style
            let default_style = include_str!("../styles/dark.json");
            serde_json::from_str(default_style)?
        };

        let styler = Arc::new(Styler::new(&style_json));
        let tile_source = Arc::new(TileSource::new(&config, styler));
        let renderer = Renderer::new(tile_source, config.clone());

        // Default zoom 0.0 shows a world overview without Mercator pole distortion.
        // set_size() will clamp up to min_zoom if terminal is too small.
        let initial_zoom = config.initial_zoom.unwrap_or(0.0);

        Ok(Self {
            center_lat: config.initial_lat,
            center_lon: config.initial_lon,
            zoom: initial_zoom,
            config,
            width: 0,
            height: 0,
            tick: 0,
            min_zoom: 0.0,
            markers: Vec::new(),
            camera: Camera::new(),
            renderer: Arc::new(Mutex::new(renderer)),
        })
    }

    /// Set the terminal display size (in pixels: width=cols*2, height=rows*4)
    pub fn set_size(&mut self, width: usize, height: usize) {
        self.width = width;
        self.height = height;
        // Min zoom: allow zooming out until the world fits in either dimension
        let min_w = (width as f64 / 256.0).log2();
        let min_h = (height as f64 / 256.0).log2();
        self.min_zoom = min_w.min(min_h);
        if self.zoom < self.min_zoom {
            self.zoom = self.min_zoom;
        }
    }

    /// Set the terminal size from column/row counts
    pub fn set_size_from_terminal(&mut self, cols: u16, rows: u16) {
        let w = (cols as usize >> 1) << 2;
        // Reserve 3 rows: 1 for footer, 2 to prevent scroll-push/truncation
        let h = (rows.saturating_sub(3) as usize) * 4;
        self.set_size(w, h);
    }

    /// Render the current view to a string frame
    pub async fn render(&self) -> anyhow::Result<String> {
        let mut renderer = self.renderer.lock().await;
        renderer.set_size(self.width, self.height);
        renderer.config.show_labels = self.config.show_labels;
        renderer.config.ocean_background = self.config.ocean_background;
        renderer
            .draw(self.center_lon, self.center_lat, self.zoom)
            .await?;

        // Draw markers on top of the tile layer
        if !self.markers.is_empty() {
            renderer.draw_markers(
                &self.markers,
                self.center_lon,
                self.center_lat,
                self.zoom,
                self.tick,
            );
        }

        Ok(renderer.frame(self.config.use_braille))
    }

    /// Zoom in/out by a step
    pub fn zoom_by(&mut self, step: f64) {
        let new_zoom = self.zoom + step;
        if new_zoom < self.min_zoom {
            self.zoom = self.min_zoom;
        } else if new_zoom > self.config.max_zoom {
            self.zoom = self.config.max_zoom;
        } else {
            self.zoom = new_zoom;
        }
    }

    /// Move the center by a lat/lon delta
    pub fn move_by(&mut self, dlat: f64, dlon: f64) {
        let (lon, lat) = utils::normalize(self.center_lon + dlon, self.center_lat + dlat);
        self.center_lon = lon;
        self.center_lat = lat;
    }

    /// Set the center to a specific lat/lon
    pub fn set_center(&mut self, lat: f64, lon: f64) {
        let (lon, lat) = utils::normalize(lon, lat);
        self.center_lon = lon;
        self.center_lat = lat;
    }

    /// Toggle between braille and ASCII rendering
    pub fn toggle_braille(&mut self) {
        self.config.use_braille = !self.config.use_braille;
    }

    /// Toggle map labels (country names, city names, POI markers) on/off
    pub fn toggle_labels(&mut self) {
        self.config.show_labels = !self.config.show_labels;
    }

    /// Toggle ocean background (blue fill for areas outside the map)
    pub fn toggle_ocean_background(&mut self) {
        self.config.ocean_background = !self.config.ocean_background;
    }

    /// Zoom and center so all meaningful landmass (72°N to 56°S) fits in the window.
    /// Computes exact zoom from the Mercator Y span of that latitude range.
    pub fn fit_world(&mut self) {
        // Latitude range covering all major landmass including Greenland
        let north = 84.0_f64;
        let south = -56.0_f64;

        // Convert to Mercator Y fractions at zoom 0 (single tile, 256px)
        let (_, y_north) = utils::ll2tile(0.0, north, 0);
        let (_, y_south) = utils::ll2tile(0.0, south, 0);
        let span_frac = y_south - y_north; // fraction of tile height needed
        let span_px = span_frac * 256.0;   // pixels at zoom 0

        // Zoom where that span fits the canvas height
        let height_zoom = (self.height as f64 / span_px).log2();
        // Also ensure the width fits (no horizontal repetition)
        let width_zoom = (self.width as f64 / 256.0).log2();
        // Use the smaller zoom so both dimensions fit
        self.zoom = height_zoom.min(width_zoom);
        if self.zoom < self.min_zoom {
            self.zoom = self.min_zoom;
        }

        // Center between north and south in Mercator space
        let y_mid = (y_north + y_south) / 2.0;
        let (_, center_lat) = utils::tile2ll(0.5, y_mid, 0);
        self.center_lat = center_lat;
        self.center_lon = 0.0;
    }

    /// Get a footer string showing current position and zoom
    pub fn footer(&self) -> String {
        format!(
            "center: {:.3}, {:.3}   zoom: {:.2}",
            utils::digits(self.center_lat, 3),
            utils::digits(self.center_lon, 3),
            utils::digits(self.zoom, 2)
        )
    }

    // ---- Marker API ----

    /// Add a marker to the map
    pub fn add_marker(&mut self, marker: MapMarker) {
        self.markers.push(marker);
    }

    /// Remove a marker by its ID
    pub fn remove_marker(&mut self, id: &str) {
        self.markers.retain(|m| m.id != id);
    }

    /// Remove all markers
    pub fn clear_markers(&mut self) {
        self.markers.clear();
    }

    /// Get a reference to all markers
    pub fn markers(&self) -> &[MapMarker] {
        &self.markers
    }

    /// Advance the animation tick counter. Call this every frame/poll cycle.
    pub fn advance_tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);
    }

    /// Returns true if any marker has an animation that needs periodic redraws
    pub fn has_animated_markers(&self) -> bool {
        self.markers.iter().any(|m| m.animation != MarkerAnimation::None)
    }

    // ---- Camera API ----

    /// Get a mutable reference to the camera controller
    pub fn camera_mut(&mut self) -> &mut Camera {
        &mut self.camera
    }

    /// Get a reference to the camera controller
    pub fn camera(&self) -> &Camera {
        &self.camera
    }

    /// Start a globe tour at country level (zoom ~2.0, shows whole countries)
    pub fn start_globe_tour(&mut self) {
        self.camera = Camera::globe_tour(2.0);
        self.camera.start(self.center_lat, self.center_lon, self.zoom);
    }

    /// Start a globe tour at the given zoom level.
    /// ~2.0 = country level, ~4.0 = city region, ~6.0 = city streets.
    pub fn start_globe_tour_at(&mut self, zoom: f64) {
        self.camera = Camera::globe_tour(zoom);
        self.camera.start(self.center_lat, self.center_lon, self.zoom);
    }

    /// Start a tour that visits each marker on the map
    pub fn start_marker_tour(&mut self, zoom: f64) {
        self.camera = Camera::from_markers(&self.markers, zoom);
        self.camera.start(self.center_lat, self.center_lon, self.zoom);
    }

    /// Toggle the camera animation on/off. Returns true if now active.
    pub fn toggle_camera(&mut self) -> bool {
        self.camera.toggle(self.center_lat, self.center_lon, self.zoom)
    }

    /// Advance the camera by one tick and update map position/zoom.
    /// Returns true if the camera is active and the view changed.
    pub fn update_camera(&mut self) -> bool {
        if let Some((lat, lon, zoom)) = self.camera.tick() {
            self.center_lat = lat;
            self.center_lon = lon;
            self.zoom = zoom.clamp(self.min_zoom, self.config.max_zoom);
            true
        } else {
            false
        }
    }

    /// Returns true if any animation (markers or camera) needs periodic redraws
    pub fn needs_animation_redraw(&self) -> bool {
        self.has_animated_markers() || self.camera.is_active()
    }
}
