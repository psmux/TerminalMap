use std::f64::consts::PI;

/// Clamp a number between min and max
pub fn clamp(num: f64, min: f64, max: f64) -> f64 {
    num.clamp(min, max)
}

/// Get the base zoom level (integer part, clamped to tile_range)
pub fn base_zoom(zoom: f64, tile_range: u32) -> u32 {
    let z = zoom.floor() as i64;
    z.max(0).min(tile_range as i64) as u32
}

/// Get the tile size at a fractional zoom level
pub fn tilesize_at_zoom(zoom: f64, project_size: f64, tile_range: u32) -> f64 {
    let bz = base_zoom(zoom, tile_range) as f64;
    project_size * 2.0_f64.powf(zoom - bz)
}

/// Convert longitude/latitude to tile coordinates
pub fn ll2tile(lon: f64, lat: f64, zoom: u32) -> (f64, f64) {
    let n = 2.0_f64.powi(zoom as i32);
    let x = (lon + 180.0) / 360.0 * n;
    let lat_rad = lat * PI / 180.0;
    let y = (1.0 - (lat_rad.tan() + 1.0 / lat_rad.cos()).ln() / PI) / 2.0 * n;
    (x, y)
}

/// Convert tile coordinates to longitude/latitude
pub fn tile2ll(x: f64, y: f64, zoom: u32) -> (f64, f64) {
    let n = PI - 2.0 * PI * y / 2.0_f64.powi(zoom as i32);
    let lon = x / 2.0_f64.powi(zoom as i32) * 360.0 - 180.0;
    let lat = 180.0 / PI * (0.5 * (n.exp() - (-n).exp())).atan();
    (lon, lat)
}

/// Normalize longitude/latitude to valid ranges
pub fn normalize(lon: f64, lat: f64) -> (f64, f64) {
    let mut lon = lon;
    let mut lat = lat;
    if lon < -180.0 {
        lon += 360.0;
    }
    if lon > 180.0 {
        lon -= 360.0;
    }
    lat = lat.clamp(-85.0511, 85.0511);
    (lon, lat)
}

/// Format a number with a given number of decimal digits
pub fn digits(number: f64, digits: u32) -> f64 {
    let factor = 10.0_f64.powi(digits as i32);
    (number * factor).floor() / factor
}

/// Convert hex color string to RGB tuple
pub fn hex2rgb(color: &str) -> (u8, u8, u8) {
    let color = color.trim_start_matches('#');
    if color.len() == 3 {
        let r = u8::from_str_radix(&color[0..1], 16).unwrap_or(0);
        let g = u8::from_str_radix(&color[1..2], 16).unwrap_or(0);
        let b = u8::from_str_radix(&color[2..3], 16).unwrap_or(0);
        (r | (r << 4), g | (g << 4), b | (b << 4))
    } else if color.len() == 6 {
        let r = u8::from_str_radix(&color[0..2], 16).unwrap_or(0);
        let g = u8::from_str_radix(&color[2..4], 16).unwrap_or(0);
        let b = u8::from_str_radix(&color[4..6], 16).unwrap_or(0);
        (r, g, b)
    } else {
        (255, 0, 0) // fallback red
    }
}

/// Convert RGB to the nearest xterm-256 color index
pub fn rgb_to_x256(r: u8, g: u8, b: u8) -> u8 {
    ansi_colours::ansi256_from_rgb((r, g, b))
}

/// Population count (number of set bits)
pub fn population(val: u32) -> u32 {
    val.count_ones()
}

/// Simplify a polyline using Ramer-Douglas-Peucker algorithm
pub fn simplify_points(points: &[(f64, f64)], tolerance: f64) -> Vec<(f64, f64)> {
    if points.len() < 3 {
        return points.to_vec();
    }
    rdp_simplify(points, tolerance)
}

fn rdp_simplify(points: &[(f64, f64)], epsilon: f64) -> Vec<(f64, f64)> {
    if points.len() < 3 {
        return points.to_vec();
    }

    let mut dmax = 0.0;
    let mut index = 0;
    let end = points.len() - 1;

    for i in 1..end {
        let d = perpendicular_distance(points[i], points[0], points[end]);
        if d > dmax {
            index = i;
            dmax = d;
        }
    }

    if dmax > epsilon {
        let mut r1 = rdp_simplify(&points[..=index], epsilon);
        let r2 = rdp_simplify(&points[index..], epsilon);
        r1.pop();
        r1.extend(r2);
        r1
    } else {
        vec![points[0], points[end]]
    }
}

fn perpendicular_distance(point: (f64, f64), line_start: (f64, f64), line_end: (f64, f64)) -> f64 {
    let dx = line_end.0 - line_start.0;
    let dy = line_end.1 - line_start.1;
    let mag = (dx * dx + dy * dy).sqrt();
    if mag < 1e-10 {
        let ddx = point.0 - line_start.0;
        let ddy = point.1 - line_start.1;
        return (ddx * ddx + ddy * ddy).sqrt();
    }
    ((point.0 - line_start.0) * dy - (point.1 - line_start.1) * dx).abs() / mag
}
