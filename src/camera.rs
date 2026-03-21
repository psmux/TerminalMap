/// Scriptable camera animation system for smooth map tours.
///
/// Supports three modes:
/// - **Globe tour**: Auto-generated world tour visiting famous cities
/// - **Markers tour**: Visits each marker on the map in sequence
/// - **Scripted**: User-defined sequence of waypoints
///
/// The camera smoothly interpolates position and zoom between waypoints
/// using ease-in-out curves for natural, cinematic movement.

/// A single waypoint the camera will travel to
#[derive(Debug, Clone)]
pub struct Waypoint {
    pub lat: f64,
    pub lon: f64,
    pub zoom: f64,
    /// How many ticks to spend traveling TO this waypoint
    pub travel_ticks: u64,
    /// How many ticks to hold/dwell at this waypoint before moving on
    pub hold_ticks: u64,
    /// Optional label shown in footer while at this waypoint
    pub label: Option<String>,
}

impl Waypoint {
    pub fn new(lat: f64, lon: f64, zoom: f64) -> Self {
        Self {
            lat,
            lon,
            zoom,
            travel_ticks: 60,  // ~3 seconds at 50ms poll
            hold_ticks: 40,    // ~2 seconds hold
            label: None,
        }
    }

    pub fn with_travel(mut self, ticks: u64) -> Self {
        self.travel_ticks = ticks;
        self
    }

    pub fn with_hold(mut self, ticks: u64) -> Self {
        self.hold_ticks = ticks;
        self
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }
}

/// Current phase of animation between two waypoints
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Traveling from previous waypoint to current
    Traveling,
    /// Holding at current waypoint
    Holding,
}

/// The camera controller that drives map animation
#[derive(Debug, Clone)]
pub struct Camera {
    waypoints: Vec<Waypoint>,
    current_index: usize,
    phase: Phase,
    phase_tick: u64,
    /// Whether to loop back to the first waypoint after the last
    pub looping: bool,
    /// Whether the camera is currently running
    active: bool,
    /// Snapshot of where we started traveling from
    from_lat: f64,
    from_lon: f64,
    from_zoom: f64,
}

impl Camera {
    /// Create a new camera with no waypoints
    pub fn new() -> Self {
        Self {
            waypoints: Vec::new(),
            current_index: 0,
            phase: Phase::Traveling,
            phase_tick: 0,
            looping: true,
            active: false,
            from_lat: 0.0,
            from_lon: 0.0,
            from_zoom: 0.0,
        }
    }

    /// Create a camera preloaded with a globe tour of famous cities.
    /// Use `zoom` to control how close to get: ~2.0 for country level, ~4.0 for
    /// city region, ~6.0 for city streets.
    pub fn globe_tour(zoom: f64) -> Self {
        let waypoints = vec![
            Waypoint::new(48.8566, 2.3522, zoom)
                .with_label("Paris")
                .with_travel(80)
                .with_hold(50),
            Waypoint::new(51.5074, -0.1278, zoom)
                .with_label("London")
                .with_travel(60)
                .with_hold(50),
            Waypoint::new(40.7128, -74.0060, zoom)
                .with_label("New York")
                .with_travel(100)
                .with_hold(50),
            Waypoint::new(34.0522, -118.2437, zoom)
                .with_label("Los Angeles")
                .with_travel(80)
                .with_hold(50),
            Waypoint::new(-22.9068, -43.1729, zoom)
                .with_label("Rio de Janeiro")
                .with_travel(100)
                .with_hold(50),
            Waypoint::new(35.6762, 139.6503, zoom)
                .with_label("Tokyo")
                .with_travel(120)
                .with_hold(50),
            Waypoint::new(-33.8688, 151.2093, zoom)
                .with_label("Sydney")
                .with_travel(80)
                .with_hold(50),
            Waypoint::new(1.3521, 103.8198, zoom)
                .with_label("Singapore")
                .with_travel(80)
                .with_hold(50),
            Waypoint::new(28.6139, 77.2090, zoom)
                .with_label("New Delhi")
                .with_travel(80)
                .with_hold(50),
            Waypoint::new(30.0444, 31.2357, zoom)
                .with_label("Cairo")
                .with_travel(80)
                .with_hold(50),
            Waypoint::new(-1.2921, 36.8219, zoom)
                .with_label("Nairobi")
                .with_travel(60)
                .with_hold(50),
            Waypoint::new(52.52, 13.405, zoom)
                .with_label("Berlin")
                .with_travel(80)
                .with_hold(50),
        ];

        let mut cam = Self::new();
        cam.waypoints = waypoints;
        cam.looping = true;
        cam
    }

    /// Create a camera that visits each marker on the map
    pub fn from_markers(markers: &[crate::marker::MapMarker], zoom: f64) -> Self {
        let waypoints: Vec<Waypoint> = markers
            .iter()
            .map(|m| {
                let label = m.label.clone().unwrap_or_else(|| m.id.clone());
                Waypoint::new(m.lat, m.lon, zoom)
                    .with_label(label)
                    .with_travel(70)
                    .with_hold(60)
            })
            .collect();

        let mut cam = Self::new();
        cam.waypoints = waypoints;
        cam.looping = true;
        cam
    }

    /// Add a waypoint to the sequence
    pub fn add_waypoint(&mut self, wp: Waypoint) {
        self.waypoints.push(wp);
    }

    /// Override the zoom level for all waypoints at once.
    /// Useful for switching an existing tour between country/city/street level.
    pub fn set_zoom(&mut self, zoom: f64) {
        for wp in &mut self.waypoints {
            wp.zoom = zoom;
        }
    }

    /// Start the camera animation from the map's current position
    pub fn start(&mut self, current_lat: f64, current_lon: f64, current_zoom: f64) {
        if self.waypoints.is_empty() {
            return;
        }
        self.active = true;
        self.current_index = 0;
        self.phase = Phase::Traveling;
        self.phase_tick = 0;
        self.from_lat = current_lat;
        self.from_lon = current_lon;
        self.from_zoom = current_zoom;
    }

    /// Stop the camera animation
    pub fn stop(&mut self) {
        self.active = false;
    }

    /// Toggle camera on/off, returns whether it is now active
    pub fn toggle(&mut self, current_lat: f64, current_lon: f64, current_zoom: f64) -> bool {
        if self.active {
            self.stop();
        } else {
            self.start(current_lat, current_lon, current_zoom);
        }
        self.active
    }

    /// Whether the camera is currently animating
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Get the label of the current target waypoint (if any)
    pub fn current_label(&self) -> Option<&str> {
        if !self.active {
            return None;
        }
        self.waypoints
            .get(self.current_index)
            .and_then(|wp| wp.label.as_deref())
    }

    /// Advance one tick and return the interpolated (lat, lon, zoom).
    /// Returns None if the camera is inactive or has no waypoints.
    pub fn tick(&mut self) -> Option<(f64, f64, f64)> {
        if !self.active || self.waypoints.is_empty() {
            return None;
        }

        let wp = &self.waypoints[self.current_index];

        match self.phase {
            Phase::Traveling => {
                self.phase_tick += 1;
                let duration = wp.travel_ticks.max(1);
                let t = (self.phase_tick as f64 / duration as f64).min(1.0);
                let eased = ease_in_out(t);

                // Interpolate position and zoom
                let lat = lerp(self.from_lat, wp.lat, eased);
                let lon = lerp_lon(self.from_lon, wp.lon, eased);
                // Zoom out slightly during travel, then back in at destination
                let mid_zoom = (self.from_zoom.min(wp.zoom) - 0.8).max(0.0);
                let zoom = if t < 0.5 {
                    let t2 = t * 2.0;
                    lerp(self.from_zoom, mid_zoom, ease_in_out(t2))
                } else {
                    let t2 = (t - 0.5) * 2.0;
                    lerp(mid_zoom, wp.zoom, ease_in_out(t2))
                };

                if self.phase_tick >= duration {
                    self.phase = Phase::Holding;
                    self.phase_tick = 0;
                }

                Some((lat, lon, zoom))
            }
            Phase::Holding => {
                self.phase_tick += 1;
                if self.phase_tick >= wp.hold_ticks {
                    // Move to next waypoint
                    self.from_lat = wp.lat;
                    self.from_lon = wp.lon;
                    self.from_zoom = wp.zoom;

                    self.current_index += 1;
                    if self.current_index >= self.waypoints.len() {
                        if self.looping {
                            self.current_index = 0;
                        } else {
                            self.active = false;
                            return None;
                        }
                    }
                    self.phase = Phase::Traveling;
                    self.phase_tick = 0;
                }

                Some((wp.lat, wp.lon, wp.zoom))
            }
        }
    }
}

/// Smooth ease-in-out (cubic)
fn ease_in_out(t: f64) -> f64 {
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

/// Linear interpolation
fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// Longitude-aware lerp that takes the shortest path around the globe
fn lerp_lon(a: f64, b: f64, t: f64) -> f64 {
    let mut diff = b - a;
    // Wrap around: take the shortest path
    if diff > 180.0 {
        diff -= 360.0;
    } else if diff < -180.0 {
        diff += 360.0;
    }
    let mut result = a + diff * t;
    // Normalize to [-180, 180]
    if result > 180.0 {
        result -= 360.0;
    } else if result < -180.0 {
        result += 360.0;
    }
    result
}
