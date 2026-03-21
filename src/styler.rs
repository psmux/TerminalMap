use serde_json::Value;
use std::collections::HashMap;

/// Represents a compiled map style, analogous to the Mapbox GL style spec.
/// Supports filter compilation for fast feature matching.
pub struct Styler {
    pub style_by_id: HashMap<String, StyleLayer>,
    pub style_by_layer: HashMap<String, Vec<StyleLayer>>,
    pub style_name: String,
}

#[derive(Debug, Clone)]
pub struct StyleLayer {
    pub id: String,
    pub layer_type: String,
    pub source_layer: Option<String>,
    pub min_zoom: Option<f64>,
    pub max_zoom: Option<f64>,
    pub filter: Option<Value>,
    pub paint: HashMap<String, Value>,
}

impl StyleLayer {
    pub fn applies_to(&self, properties: &HashMap<String, Value>) -> bool {
        match &self.filter {
            Some(filter) => evaluate_filter(filter, properties),
            None => true,
        }
    }
}

fn evaluate_filter(filter: &Value, properties: &HashMap<String, Value>) -> bool {
    match filter.as_array() {
        Some(arr) if !arr.is_empty() => {
            let op = arr[0].as_str().unwrap_or("");
            match op {
                "all" => arr[1..].iter().all(|f| evaluate_filter(f, properties)),
                "any" => arr[1..].iter().any(|f| evaluate_filter(f, properties)),
                "none" => !arr[1..].iter().any(|f| evaluate_filter(f, properties)),
                "==" => {
                    if arr.len() >= 3 {
                        let key = arr[1].as_str().unwrap_or("");
                        let expected = &arr[2];
                        properties.get(key).map_or(false, |v| v == expected)
                    } else {
                        true
                    }
                }
                "!=" => {
                    if arr.len() >= 3 {
                        let key = arr[1].as_str().unwrap_or("");
                        let expected = &arr[2];
                        properties.get(key).map_or(true, |v| v != expected)
                    } else {
                        true
                    }
                }
                "in" => {
                    if arr.len() >= 3 {
                        let key = arr[1].as_str().unwrap_or("");
                        let val = properties.get(key);
                        arr[2..].iter().any(|v| val.map_or(false, |pv| pv == v))
                    } else {
                        true
                    }
                }
                "!in" => {
                    if arr.len() >= 3 {
                        let key = arr[1].as_str().unwrap_or("");
                        let val = properties.get(key);
                        !arr[2..].iter().any(|v| val.map_or(false, |pv| pv == v))
                    } else {
                        true
                    }
                }
                "has" => {
                    if arr.len() >= 2 {
                        let key = arr[1].as_str().unwrap_or("");
                        properties.contains_key(key)
                    } else {
                        true
                    }
                }
                "!has" => {
                    if arr.len() >= 2 {
                        let key = arr[1].as_str().unwrap_or("");
                        !properties.contains_key(key)
                    } else {
                        true
                    }
                }
                ">" => compare_filter(properties, &arr, |a, b| a > b),
                ">=" => compare_filter(properties, &arr, |a, b| a >= b),
                "<" => compare_filter(properties, &arr, |a, b| a < b),
                "<=" => compare_filter(properties, &arr, |a, b| a <= b),
                _ => true,
            }
        }
        _ => true,
    }
}

fn compare_filter(
    properties: &HashMap<String, Value>,
    arr: &[Value],
    cmp: fn(f64, f64) -> bool,
) -> bool {
    if arr.len() >= 3 {
        let key = arr[1].as_str().unwrap_or("");
        let threshold = arr[2].as_f64().unwrap_or(0.0);
        properties
            .get(key)
            .and_then(|v| v.as_f64())
            .map_or(false, |v| cmp(v, threshold))
    } else {
        true
    }
}

impl Styler {
    pub fn new(style_json: &Value) -> Self {
        let style_name = style_json["name"].as_str().unwrap_or("").to_string();
        let constants = style_json.get("constants").cloned();
        let layers_arr = style_json["layers"].as_array().cloned().unwrap_or_default();

        let mut style_by_id: HashMap<String, StyleLayer> = HashMap::new();
        let mut style_by_layer: HashMap<String, Vec<StyleLayer>> = HashMap::new();

        for layer_val in &layers_arr {
            let mut layer_val = layer_val.clone();

            // Replace constants
            if let Some(ref consts) = constants {
                replace_constants(consts, &mut layer_val);
            }

            let id = layer_val["id"].as_str().unwrap_or("").to_string();

            // Resolve references
            if let Some(ref_id) = layer_val.get("ref").and_then(|v| v.as_str()) {
                if let Some(ref_layer) = style_by_id.get(ref_id) {
                    let fields = ["type", "source-layer", "minzoom", "maxzoom", "filter"];
                    for field in &fields {
                        if layer_val.get(*field).is_none() || layer_val[*field].is_null() {
                            match *field {
                                "type" => {
                                    layer_val[*field] =
                                        Value::String(ref_layer.layer_type.clone());
                                }
                                "source-layer" => {
                                    if let Some(ref sl) = ref_layer.source_layer {
                                        layer_val[*field] = Value::String(sl.clone());
                                    }
                                }
                                "minzoom" => {
                                    if let Some(z) = ref_layer.min_zoom {
                                        layer_val[*field] =
                                            serde_json::Number::from_f64(z)
                                                .map(Value::Number)
                                                .unwrap_or(Value::Null);
                                    }
                                }
                                "maxzoom" => {
                                    if let Some(z) = ref_layer.max_zoom {
                                        layer_val[*field] =
                                            serde_json::Number::from_f64(z)
                                                .map(Value::Number)
                                                .unwrap_or(Value::Null);
                                    }
                                }
                                "filter" => {
                                    if let Some(ref f) = ref_layer.filter {
                                        layer_val[*field] = f.clone();
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }

            let layer_type = layer_val["type"].as_str().unwrap_or("").to_string();
            let source_layer = layer_val["source-layer"]
                .as_str()
                .map(|s| s.to_string());
            let min_zoom = layer_val["minzoom"].as_f64();
            let max_zoom = layer_val["maxzoom"].as_f64();
            let filter = layer_val.get("filter").cloned();

            let paint = if let Some(paint_obj) = layer_val.get("paint").and_then(|v| v.as_object())
            {
                paint_obj
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect()
            } else {
                HashMap::new()
            };

            let style_layer = StyleLayer {
                id: id.clone(),
                layer_type,
                source_layer: source_layer.clone(),
                min_zoom,
                max_zoom,
                filter,
                paint,
            };

            style_by_id.insert(id, style_layer.clone());

            if let Some(ref sl) = source_layer {
                style_by_layer
                    .entry(sl.clone())
                    .or_default()
                    .push(style_layer);
            }
        }

        Self {
            style_by_id,
            style_by_layer,
            style_name,
        }
    }

    /// Get the first matching style for a feature in a given layer
    pub fn get_style_for(
        &self,
        layer: &str,
        properties: &HashMap<String, Value>,
    ) -> Option<&StyleLayer> {
        self.style_by_layer.get(layer).and_then(|styles| {
            styles.iter().find(|s| s.applies_to(properties))
        })
    }
}

fn replace_constants(constants: &Value, node: &mut Value) {
    match node {
        Value::Object(map) => {
            for (_, v) in map.iter_mut() {
                replace_constants(constants, v);
            }
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                replace_constants(constants, v);
            }
        }
        Value::String(s) => {
            if s.starts_with('@') {
                if let Some(replacement) = constants.get(s.as_str()) {
                    *node = replacement.clone();
                }
            }
        }
        _ => {}
    }
}
