use crate::utils;

/// Visual style for a map marker's animation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerAnimation {
    /// Static marker, always visible
    None,
    /// Blinks on/off at the given interval
    Blink,
    /// Pulses between bright and dim (cycles through ring sizes)
    Pulse,
    /// Flashes rapidly
    Flash,
}

/// Shape of the marker rendered on the map
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerShape {
    /// Single braille dot
    Dot,
    /// Small cross (+)
    Cross,
    /// Diamond shape
    Diamond,
    /// Circle ring (radius in pixels)
    Ring(u8),
    /// Filled circle (radius in pixels)
    FilledCircle(u8),
    /// Unicode character
    Char(char),
}

/// A point to be plotted on the map
#[derive(Debug, Clone)]
pub struct MapMarker {
    /// Latitude of the marker
    pub lat: f64,
    /// Longitude of the marker
    pub lon: f64,
    /// Display label (shown next to marker)
    pub label: Option<String>,
    /// xterm-256 color index (use `utils::rgb_to_x256` to convert)
    pub color: u8,
    /// Visual shape
    pub shape: MarkerShape,
    /// Animation style
    pub animation: MarkerAnimation,
    /// Unique ID for the marker
    pub id: String,
}

impl MapMarker {
    /// Create a simple colored dot marker
    pub fn dot(lat: f64, lon: f64, color: u8) -> Self {
        Self {
            lat,
            lon,
            label: None,
            color,
            shape: MarkerShape::Dot,
            animation: MarkerAnimation::None,
            id: format!("{:.6},{:.6}", lat, lon),
        }
    }

    /// Create a marker with a label
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Set the animation style
    pub fn with_animation(mut self, animation: MarkerAnimation) -> Self {
        self.animation = animation;
        self
    }

    /// Set the shape
    pub fn with_shape(mut self, shape: MarkerShape) -> Self {
        self.shape = shape;
        self
    }

    /// Set a custom ID
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = id.into();
        self
    }

    /// Create a marker from RGB color values
    pub fn dot_rgb(lat: f64, lon: f64, r: u8, g: u8, b: u8) -> Self {
        Self::dot(lat, lon, utils::rgb_to_x256(r, g, b))
    }
}

/// Determines visibility of animated markers for a given frame tick
pub fn marker_visible(animation: MarkerAnimation, tick: u64) -> bool {
    match animation {
        MarkerAnimation::None => true,
        MarkerAnimation::Blink => (tick / 8) % 2 == 0, // ~400ms on/off at 50ms poll
        MarkerAnimation::Pulse => true, // always visible, size changes
        MarkerAnimation::Flash => (tick / 3) % 2 == 0, // ~150ms on/off, rapid
    }
}

/// For pulse animation, returns a ring radius offset for the current tick
pub fn pulse_radius(tick: u64) -> u8 {
    let phase = (tick / 4) % 6; // cycle through 0..5
    match phase {
        0 => 1,
        1 => 2,
        2 => 3,
        3 => 4,
        4 => 3,
        5 => 2,
        _ => 1,
    }
}
