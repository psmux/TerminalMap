/// Configuration for the map viewer
#[derive(Debug, Clone)]
pub struct MapConfig {
    pub language: String,
    pub source: String,
    pub style_data: Option<String>,
    pub initial_zoom: Option<f64>,
    pub max_zoom: f64,
    pub zoom_step: f64,
    pub initial_lat: f64,
    pub initial_lon: f64,
    pub simplify_polylines: bool,
    pub use_braille: bool,
    pub persist_downloaded_tiles: bool,
    pub tile_range: u32,
    pub project_size: f64,
    pub label_margin: f64,
    pub poi_marker: char,
    pub show_labels: bool,
    pub ocean_background: bool,
}

impl Default for MapConfig {
    fn default() -> Self {
        Self {
            language: "en".to_string(),
            source: "https://tiles.openfreemap.org/planet".to_string(),
            style_data: None,
            initial_zoom: None,
            max_zoom: 18.0,
            zoom_step: 0.2,
            initial_lat: 52.51298,
            initial_lon: 13.42012,
            simplify_polylines: false,
            use_braille: true,
            persist_downloaded_tiles: true,
            tile_range: 14,
            project_size: 256.0,
            label_margin: 5.0,
            poi_marker: '\u{25C9}', // ◉
            show_labels: true,
            ocean_background: true,
        }
    }
}
