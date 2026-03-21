use crate::proto;
use crate::styler::Styler;
use crate::utils;

use anyhow::Result;
use flate2::read::GzDecoder;
use prost::Message;
use serde_json::Value;
use std::collections::HashMap;
use std::io::Read;

/// Decoded geometry point
#[derive(Debug, Clone)]
pub struct GeomPoint {
    pub x: i32,
    pub y: i32,
}

/// A decoded and styled feature from a vector tile
#[derive(Debug, Clone)]
pub struct TileFeature {
    pub layer_name: String,
    pub style_type: String,
    pub color: u8,
    pub label: Option<String>,
    pub sort: i64,
    pub points: Vec<Vec<GeomPoint>>,
    pub min_x: i32,
    pub max_x: i32,
    pub min_y: i32,
    pub max_y: i32,
    pub min_zoom: Option<f64>,
    pub max_zoom: Option<f64>,
    pub line_width: f64,
}

/// A layer in the tile with its features and extent
#[derive(Debug, Clone)]
pub struct TileLayer {
    pub extent: u32,
    pub features: Vec<TileFeature>,
}

/// A parsed vector tile
#[derive(Debug, Clone)]
pub struct ParsedTile {
    pub layers: HashMap<String, TileLayer>,
}

/// Decode and process a vector tile from raw bytes
pub fn load_tile(buffer: &[u8], styler: &Styler, language: &str) -> Result<ParsedTile> {
    let decompressed = decompress_if_needed(buffer)?;
    let tile = proto::Tile::decode(decompressed.as_slice())?;
    let layers = load_layers(&tile, styler, language);
    Ok(ParsedTile { layers })
}

fn decompress_if_needed(buffer: &[u8]) -> Result<Vec<u8>> {
    if buffer.len() >= 2 && buffer[0] == 0x1f && buffer[1] == 0x8b {
        let mut decoder = GzDecoder::new(buffer);
        let mut decompressed = Vec::new();
        decoder.read_to_end(&mut decompressed)?;
        Ok(decompressed)
    } else {
        Ok(buffer.to_vec())
    }
}

/// Remap OpenMapTiles layer names and properties to Mapbox Streets v6 equivalents
/// so existing styles (dark.json, bright.json) work with modern tile sources.
fn remap_openmaptiles<'a>(
    layer_name: &'a str,
    properties: &mut HashMap<String, Value>,
) -> &'a str {
    // Normalize name:xx properties to name_xx (OpenMapTiles uses colons, styles expect underscores)
    let colon_keys: Vec<String> = properties
        .keys()
        .filter(|k| k.starts_with("name:"))
        .cloned()
        .collect();
    for key in colon_keys {
        let new_key = key.replace(':', "_");
        if let Some(val) = properties.get(&key).cloned() {
            properties.insert(new_key, val);
        }
    }

    match layer_name {
        "transportation" => {
            // brunnel -> structure (bridge/tunnel classification)
            if let Some(brunnel) = properties.get("brunnel").cloned() {
                properties.insert("structure".to_string(), brunnel);
            }
            // ramp=1 with class=motorway -> motorway_link
            let is_ramp = properties
                .get("ramp")
                .and_then(|v| v.as_i64())
                .unwrap_or(0)
                == 1;
            if is_ramp {
                if let Some(class) = properties.get("class").and_then(|v| v.as_str()) {
                    let link_class = format!("{}_link", class);
                    properties.insert("class".to_string(), Value::String(link_class));
                }
            }
            // minor -> street
            if properties.get("class").and_then(|v| v.as_str()) == Some("minor") {
                properties.insert("class".to_string(), Value::String("street".to_string()));
            }
            "road"
        }
        "boundary" => "admin",
        "transportation_name" => "road_label",
        "place" => {
            // Map class -> type for place_label filter compat
            if let Some(class_val) = properties.get("class").cloned() {
                properties.insert("type".to_string(), class_val);
            }
            // rank -> scalerank
            if let Some(rank) = properties.get("rank").cloned() {
                properties.insert("scalerank".to_string(), rank);
            }
            let class = properties
                .get("class")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match class {
                "country" => "country_label",
                _ => "place_label",
            }
        }
        "water_name" => {
            if let Some(rank) = properties.get("rank").cloned() {
                properties.insert("labelrank".to_string(), rank);
            }
            let class = properties
                .get("class")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match class {
                "ocean" | "sea" => "marine_label",
                _ => "water_label",
            }
        }
        "poi" => {
            if let Some(rank) = properties.get("rank").cloned() {
                properties.insert("scalerank".to_string(), rank);
            }
            "poi_label"
        }
        "aerodrome_label" => {
            if let Some(rank) = properties.get("rank").cloned() {
                properties.insert("scalerank".to_string(), rank);
            }
            "airport_label"
        }
        "park" => "landuse_overlay",
        "landcover" => "landuse",
        // water, waterway, building, landuse, aeroway, admin, road etc. stay as-is
        _ => layer_name,
    }
}

fn load_layers(
    tile: &proto::Tile,
    styler: &Styler,
    language: &str,
) -> HashMap<String, TileLayer> {
    let mut layers = HashMap::new();
    let mut color_cache: HashMap<String, u8> = HashMap::new();

    for layer in &tile.layers {
        let raw_name = &layer.name;
        let extent = layer.extent.unwrap_or(4096);

        for feature in &layer.features {
            let geom_type = feature.r#type.unwrap_or(0);
            let type_str = match geom_type {
                1 => "Point",
                2 => "LineString",
                3 => "Polygon",
                _ => "Unknown",
            };

            // Decode feature properties
            let mut properties: HashMap<String, Value> = HashMap::new();
            properties.insert("$type".to_string(), Value::String(type_str.to_string()));

            let mut tag_idx = 0;
            while tag_idx + 1 < feature.tags.len() {
                let key_idx = feature.tags[tag_idx] as usize;
                let val_idx = feature.tags[tag_idx + 1] as usize;

                if key_idx < layer.keys.len() && val_idx < layer.values.len() {
                    let key = &layer.keys[key_idx];
                    let val = &layer.values[val_idx];
                    let json_val = proto_value_to_json(val);
                    properties.insert(key.clone(), json_val);
                }
                tag_idx += 2;
            }

            // Remap OpenMapTiles layer/property names to Mapbox Streets equivalents
            let name = remap_openmaptiles(raw_name, &mut properties);

            // Get style (try remapped name first, fall back to raw name)
            let style = match styler.get_style_for(name, &properties) {
                Some(s) => s,
                None => match styler.get_style_for(raw_name, &properties) {
                    Some(s) => s,
                    None => continue,
                },
            };

            // Determine color
            let color_str = style
                .paint
                .get("line-color")
                .or_else(|| style.paint.get("fill-color"))
                .or_else(|| style.paint.get("text-color"))
                .and_then(|v| {
                    if let Value::String(s) = v {
                        Some(s.clone())
                    } else if let Value::Object(obj) = v {
                        // Handle zoom stops
                        obj.get("stops")
                            .and_then(|s| s.as_array())
                            .and_then(|arr| arr.first())
                            .and_then(|pair| pair.as_array())
                            .and_then(|p| p.get(1))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| "#f00".to_string());

            let color_code = *color_cache.entry(color_str.clone()).or_insert_with(|| {
                let (r, g, b) = utils::hex2rgb(&color_str);
                utils::rgb_to_x256(r, g, b)
            });

            // Decode geometry
            let geometries = decode_geometry(feature);

            // Extract labels for symbol layers
            let label = if style.layer_type == "symbol" {
                let lang_key = format!("name_{}", language);
                let lang_key_colon = format!("name:{}", language);
                properties
                    .get(&lang_key)
                    .or_else(|| properties.get(&lang_key_colon))
                    .or_else(|| properties.get("name_en"))
                    .or_else(|| properties.get("name:en"))
                    .or_else(|| properties.get("name"))
                    .or_else(|| properties.get("house_num"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            } else {
                None
            };

            let sort = properties
                .get("localrank")
                .or_else(|| properties.get("scalerank"))
                .and_then(|v| v.as_i64())
                .unwrap_or(0);

            // Get line width
            let line_width = style
                .paint
                .get("line-width")
                .map(|v| {
                    if let Some(n) = v.as_f64() {
                        n
                    } else if let Value::Object(obj) = v {
                        obj.get("stops")
                            .and_then(|s| s.as_array())
                            .and_then(|arr| arr.first())
                            .and_then(|pair| pair.as_array())
                            .and_then(|p| p.get(1))
                            .and_then(|v| v.as_f64())
                            .unwrap_or(1.0)
                    } else {
                        1.0
                    }
                })
                .unwrap_or(1.0);

            let entry = layers
                .entry(name.to_string())
                .or_insert_with(|| TileLayer {
                    extent,
                    features: Vec::new(),
                });

            if style.layer_type == "fill" {
                // Classify rings into polygon groups by winding order.
                // In MVT, each clockwise (outer) ring starts a new polygon;
                // subsequent counter-clockwise rings are holes in it.
                let polygon_groups = classify_rings(&geometries);
                for group in polygon_groups {
                    let (min_x, max_x, min_y, max_y) = compute_bounds_deep(&group);
                    entry.features.push(TileFeature {
                        layer_name: name.to_string(),
                        style_type: style.layer_type.clone(),
                        color: color_code,
                        label: label.clone(),
                        sort,
                        points: group,
                        min_x,
                        max_x,
                        min_y,
                        max_y,
                        min_zoom: style.min_zoom,
                        max_zoom: style.max_zoom,
                        line_width,
                    });
                }
            } else {
                // For line/symbol, each geometry is separate
                for geom in geometries {
                    let (min_x, max_x, min_y, max_y) = compute_bounds(&geom);
                    entry.features.push(TileFeature {
                        layer_name: name.to_string(),
                        style_type: style.layer_type.clone(),
                        color: color_code,
                        label: label.clone(),
                        sort,
                        points: vec![geom],
                        min_x,
                        max_x,
                        min_y,
                        max_y,
                        min_zoom: style.min_zoom,
                        max_zoom: style.max_zoom,
                        line_width,
                    });
                }
            }
        }
    }
    layers
}

fn proto_value_to_json(val: &proto::tile::Value) -> Value {
    if let Some(ref s) = val.string_value {
        Value::String(s.clone())
    } else if let Some(f) = val.float_value {
        serde_json::Number::from_f64(f as f64)
            .map(Value::Number)
            .unwrap_or(Value::Null)
    } else if let Some(d) = val.double_value {
        serde_json::Number::from_f64(d)
            .map(Value::Number)
            .unwrap_or(Value::Null)
    } else if let Some(i) = val.int_value {
        Value::Number(serde_json::Number::from(i))
    } else if let Some(u) = val.uint_value {
        Value::Number(serde_json::Number::from(u))
    } else if let Some(s) = val.sint_value {
        Value::Number(serde_json::Number::from(s))
    } else if let Some(b) = val.bool_value {
        Value::Bool(b)
    } else {
        Value::Null
    }
}

/// Decode MVT geometry commands into point lists
fn decode_geometry(feature: &proto::tile::Feature) -> Vec<Vec<GeomPoint>> {
    let mut geometries: Vec<Vec<GeomPoint>> = Vec::new();
    let mut current: Vec<GeomPoint> = Vec::new();
    let cmds = &feature.geometry;
    let mut i = 0;
    let mut cx: i32 = 0;
    let mut cy: i32 = 0;

    while i < cmds.len() {
        let cmd_int = cmds[i];
        let cmd = cmd_int & 0x7;
        let count = (cmd_int >> 3) as usize;
        i += 1;

        match cmd {
            1 => {
                // MoveTo
                for _ in 0..count {
                    if i + 1 >= cmds.len() {
                        break;
                    }
                    let dx = decode_zigzag(cmds[i]);
                    let dy = decode_zigzag(cmds[i + 1]);
                    i += 2;
                    cx += dx;
                    cy += dy;
                    if !current.is_empty() {
                        geometries.push(std::mem::take(&mut current));
                    }
                    current.push(GeomPoint { x: cx, y: cy });
                }
            }
            2 => {
                // LineTo
                for _ in 0..count {
                    if i + 1 >= cmds.len() {
                        break;
                    }
                    let dx = decode_zigzag(cmds[i]);
                    let dy = decode_zigzag(cmds[i + 1]);
                    i += 2;
                    cx += dx;
                    cy += dy;
                    current.push(GeomPoint { x: cx, y: cy });
                }
            }
            7 => {
                // ClosePath
                if let Some(first) = current.first() {
                    let first = first.clone();
                    current.push(first);
                }
            }
            _ => {}
        }
    }

    if !current.is_empty() {
        geometries.push(current);
    }

    geometries
}

fn decode_zigzag(val: u32) -> i32 {
    ((val >> 1) as i32) ^ (-((val & 1) as i32))
}

/// Compute the signed area of a ring (2x actual area).
/// Positive = clockwise in screen coords (Y down) = outer ring in MVT.
/// Negative = counter-clockwise = hole.
fn signed_area(ring: &[GeomPoint]) -> f64 {
    let n = ring.len();
    if n < 3 {
        return 0.0;
    }
    let mut sum = 0.0_f64;
    let mut j = n - 1;
    for i in 0..n {
        sum += (ring[j].x as f64 - ring[i].x as f64) * (ring[j].y as f64 + ring[i].y as f64);
        j = i;
    }
    sum
}

/// Classify decoded geometry rings into polygon groups.
/// Each group is [outer_ring, hole1, hole2, ...].
/// An outer ring has positive signed area (CW in MVT screen coords).
fn classify_rings(rings: &[Vec<GeomPoint>]) -> Vec<Vec<Vec<GeomPoint>>> {
    let mut polygons: Vec<Vec<Vec<GeomPoint>>> = Vec::new();

    for ring in rings {
        let area = signed_area(ring);
        if area >= 0.0 {
            // Outer ring (clockwise) starts a new polygon
            polygons.push(vec![ring.clone()]);
        } else if let Some(last) = polygons.last_mut() {
            // Hole (counter-clockwise) belongs to the last outer ring
            last.push(ring.clone());
        } else {
            // Orphan hole with no outer ring; treat as its own polygon
            polygons.push(vec![ring.clone()]);
        }
    }

    // If classification produced nothing (all rings had zero area or were empty),
    // fall back to treating the entire set as one polygon.
    if polygons.is_empty() && !rings.is_empty() {
        polygons.push(rings.to_vec());
    }

    polygons
}

fn compute_bounds(points: &[GeomPoint]) -> (i32, i32, i32, i32) {
    let mut min_x = i32::MAX;
    let mut max_x = i32::MIN;
    let mut min_y = i32::MAX;
    let mut max_y = i32::MIN;
    for p in points {
        if p.x < min_x { min_x = p.x; }
        if p.x > max_x { max_x = p.x; }
        if p.y < min_y { min_y = p.y; }
        if p.y > max_y { max_y = p.y; }
    }
    (min_x, max_x, min_y, max_y)
}

fn compute_bounds_deep(rings: &[Vec<GeomPoint>]) -> (i32, i32, i32, i32) {
    let mut min_x = i32::MAX;
    let mut max_x = i32::MIN;
    let mut min_y = i32::MAX;
    let mut max_y = i32::MIN;
    for ring in rings {
        for p in ring {
            if p.x < min_x { min_x = p.x; }
            if p.x > max_x { max_x = p.x; }
            if p.y < min_y { min_y = p.y; }
            if p.y > max_y { max_y = p.y; }
        }
    }
    if min_x == i32::MAX {
        (0, 0, 0, 0)
    } else {
        (min_x, max_x, min_y, max_y)
    }
}
