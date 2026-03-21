/// Embedded low-zoom tiles for offline rendering (zoom 0-1, ~480KB gzipped).
/// These are OpenMapTiles-schema PBF tiles from OpenFreeMap, gzip-compressed.

// z=0
const Z0_0_0: &[u8] = include_bytes!("../tiles_embedded/0_0_0.pbf.gz");
// z=1
const Z1_0_0: &[u8] = include_bytes!("../tiles_embedded/1_0_0.pbf.gz");
const Z1_0_1: &[u8] = include_bytes!("../tiles_embedded/1_0_1.pbf.gz");
const Z1_1_0: &[u8] = include_bytes!("../tiles_embedded/1_1_0.pbf.gz");
const Z1_1_1: &[u8] = include_bytes!("../tiles_embedded/1_1_1.pbf.gz");

/// Returns embedded tile data (gzipped PBF) for the given z/x/y, if available.
pub fn get(z: u32, x: i32, y: i32) -> Option<&'static [u8]> {
    match (z, x, y) {
        (0, 0, 0) => Some(Z0_0_0),
        (1, 0, 0) => Some(Z1_0_0),
        (1, 0, 1) => Some(Z1_0_1),
        (1, 1, 0) => Some(Z1_1_0),
        (1, 1, 1) => Some(Z1_1_1),
        _ => None,
    }
}
