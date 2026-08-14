use std::fs;
use std::path::{Path, PathBuf};

use myroute_core::{Coordinate, RoadGraph, Route, RouteComparison};
use serde_json::{Value, json};

use crate::CliError;

const METERS_PER_MILE: f64 = 1_609.344;
const LEAFLET_LAYOUT_CSS: &str = include_str!("../assets/leaflet-layout.css");

pub(crate) fn terminal_summary(
    from: Coordinate,
    to: Coordinate,
    comparison: &RouteComparison,
) -> String {
    let fastest = &comparison.fastest;
    let personal = &comparison.personalized;
    let time_delta = (personal.travel_time_s - fastest.travel_time_s) / 60.0;
    let distance_delta = miles(personal.distance_m - fastest.distance_m);
    let from = coordinate_label(from);
    let to = coordinate_label(to);
    format!(
        "MyRoute\n{from} -> {to}\n\n\
         Fastest route:\n  {:>8.1} min\n  {:>8.1} mi\n\n\
         Personalized route:\n  {:>8.1} min\n  {:>8.1} mi\n\n\
         Difference:\n  {time_delta:>+8.1} min\n  {distance_delta:>+8.1} mi\n\n\
         Route characteristics:\n\
                             Fastest     MyRoute\n\
         Highway distance  {:>8.1} mi  {:>8.1} mi\n\
         Traffic lights    {:>8}     {:>8}\n\
         Left turns        {:>8}     {:>8}\n\
         Signalized lefts  {:>8}     {:>8}\n\
         Right turns       {:>8}     {:>8}\n\
         U-turns           {:>8}     {:>8}\n\
         Minor roads       {:>8.1} mi  {:>8.1} mi\n\n\
         Preference score:\n  Fastest: {:.2}\n  MyRoute: {:.2}\n\n{}",
        fastest.travel_time_s / 60.0,
        miles(fastest.distance_m),
        personal.travel_time_s / 60.0,
        miles(personal.distance_m),
        miles(fastest.stats.highway_distance_m),
        miles(personal.stats.highway_distance_m),
        fastest.stats.traffic_signal_count,
        personal.stats.traffic_signal_count,
        fastest.stats.left_turn_count,
        personal.stats.left_turn_count,
        fastest.stats.signalized_left_turn_count,
        personal.stats.signalized_left_turn_count,
        fastest.stats.right_turn_count,
        personal.stats.right_turn_count,
        fastest.stats.u_turn_count,
        personal.stats.u_turn_count,
        miles(fastest.stats.minor_road_distance_m),
        miles(personal.stats.minor_road_distance_m),
        comparison.fastest_preference_score,
        comparison.personalized_preference_score,
        explanation(comparison),
    )
}

pub(crate) fn write_map_outputs(
    directory: &Path,
    graph: &RoadGraph,
    requested_from: Coordinate,
    requested_to: Coordinate,
    snapped_from: Coordinate,
    snapped_to: Coordinate,
    comparison: &RouteComparison,
) -> Result<(), CliError> {
    fs::create_dir_all(directory)
        .map_err(|source| io_error("create output directory", directory, source))?;
    let geojson = build_geojson(
        graph,
        requested_from,
        requested_to,
        snapped_from,
        snapped_to,
        comparison,
    )?;
    let geojson_text = serde_json::to_string_pretty(&geojson)?;
    let geojson_path = directory.join("route.geojson");
    fs::write(&geojson_path, &geojson_text)
        .map_err(|source| io_error("write GeoJSON", &geojson_path, source))?;
    let html_path = directory.join("route.html");
    fs::write(&html_path, html_document(&geojson)?)
        .map_err(|source| io_error("write HTML map", &html_path, source))?;
    Ok(())
}

fn build_geojson(
    graph: &RoadGraph,
    requested_from: Coordinate,
    requested_to: Coordinate,
    snapped_from: Coordinate,
    snapped_to: Coordinate,
    comparison: &RouteComparison,
) -> Result<Value, CliError> {
    Ok(json!({
        "type": "FeatureCollection",
        "features": [
            route_feature("fastest", &comparison.fastest, graph, snapped_from)?,
            route_feature("personalized", &comparison.personalized, graph, snapped_from)?,
            point_feature("requested_origin", requested_from),
            point_feature("requested_destination", requested_to),
            point_feature("snapped_origin", snapped_from),
            point_feature("snapped_destination", snapped_to),
        ]
    }))
}

fn route_feature(
    kind: &str,
    route: &Route,
    graph: &RoadGraph,
    snapped: Coordinate,
) -> Result<Value, CliError> {
    let route_geometry = if route.edges.is_empty() {
        json!({ "type": "Point", "coordinates": position(snapped) })
    } else {
        let coordinates = route
            .geometry(graph)?
            .into_iter()
            .map(position)
            .collect::<Vec<_>>();
        json!({ "type": "LineString", "coordinates": coordinates })
    };
    Ok(json!({
        "type": "Feature",
        "properties": {
            "kind": kind,
            "travel_time_s": route.travel_time_s,
            "distance_m": route.distance_m,
            "personalized_cost": route.personalized_cost,
            "highway_distance_m": route.stats.highway_distance_m,
            "minor_road_distance_m": route.stats.minor_road_distance_m,
            "traffic_signal_count": route.stats.traffic_signal_count,
            "left_turn_count": route.stats.left_turn_count,
            "right_turn_count": route.stats.right_turn_count,
            "u_turn_count": route.stats.u_turn_count,
            "signalized_left_turn_count": route.stats.signalized_left_turn_count,
            "unsignalized_left_turn_count": route.stats.unsignalized_left_turn_count,
            "general_turn_penalty_s": route.stats.general_turn_penalty_s,
            "left_turn_penalty_s": route.stats.left_turn_penalty_s,
            "u_turn_penalty_s": route.stats.u_turn_penalty_s,
        },
        "geometry": route_geometry,
    }))
}

fn point_feature(kind: &str, coordinate: Coordinate) -> Value {
    json!({
        "type": "Feature",
        "properties": { "kind": kind },
        "geometry": { "type": "Point", "coordinates": position(coordinate) }
    })
}

fn html_document(geojson: &Value) -> Result<String, CliError> {
    let data = escape_script_data(&serde_json::to_string(geojson)?);
    let leaflet_layout_css = LEAFLET_LAYOUT_CSS;
    Ok(format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>MyRoute</title>
  <style>{leaflet_layout_css}</style>
</head>
<body>
  <div id="map"></div>
  <script id="route-data" type="application/json">{data}</script>
  <script src="https://unpkg.com/leaflet@1.9.4/dist/leaflet.js" integrity="sha256-20nQCchB9co0qIjJZRGuk2/Z9VM+kNiyxNV1lvTlZBo=" crossorigin=""></script>
  <script>
    const data = JSON.parse(document.getElementById('route-data').textContent);
    const map = L.map('map');
    L.tileLayer('https://tile.openstreetmap.org/{{z}}/{{x}}/{{y}}.png', {{maxZoom: 19, attribution: '&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a> contributors'}}).addTo(map);
    const colors = {{fastest:'#2563eb', personalized:'#dc2626'}};
    const layer = L.geoJSON(data, {{
      style: feature => ({{color: colors[feature.properties.kind] || '#555', weight: feature.properties.kind === 'personalized' ? 6 : 4, opacity: 0.8}}),
      pointToLayer: (feature, latlng) => L.circleMarker(latlng, {{radius: feature.properties.kind.startsWith('requested') ? 5 : 8, color: colors[feature.properties.kind] || (feature.properties.kind.includes('origin') ? '#16a34a' : '#7c3aed'), fillOpacity: 0.85}}),
      onEachFeature: (feature, item) => item.bindPopup(feature.properties.kind.replaceAll('_', ' '))
    }}).addTo(map);
    const bounds = layer.getBounds();
    if (bounds.isValid()) map.fitBounds(bounds.pad(0.1)); else map.setView([0, 0], 2);
    const legend = L.control({{position:'topright'}});
    legend.onAdd = () => {{ const div=L.DomUtil.create('div','legend'); div.innerHTML='<b>MyRoute</b><br><span class="swatch" style="background:#2563eb"></span>Fastest<br><span class="swatch" style="background:#dc2626"></span>Personalized'; return div; }};
    legend.addTo(map);
  </script>
</body>
</html>
"#
    ))
}

fn escape_script_data(input: &str) -> String {
    input
        .replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

fn explanation(comparison: &RouteComparison) -> String {
    let fast = &comparison.fastest;
    let personal = &comparison.personalized;
    if fast.edges == personal.edges {
        return "MyRoute matches the fastest route for these preferences and time limit.".into();
    }
    let mut changes = vec![format!(
        "{:+.1} min travel time",
        (personal.travel_time_s - fast.travel_time_s) / 60.0
    )];
    push_distance_change(
        &mut changes,
        "mi total distance",
        personal.distance_m - fast.distance_m,
    );
    push_distance_change(
        &mut changes,
        "mi highway distance",
        personal.stats.highway_distance_m - fast.stats.highway_distance_m,
    );
    push_count_change(
        &mut changes,
        "traffic lights",
        personal.stats.traffic_signal_count,
        fast.stats.traffic_signal_count,
    );
    push_count_change(
        &mut changes,
        "left turns",
        personal.stats.left_turn_count,
        fast.stats.left_turn_count,
    );
    push_count_change(
        &mut changes,
        "right turns",
        personal.stats.right_turn_count,
        fast.stats.right_turn_count,
    );
    push_count_change(
        &mut changes,
        "U-turns",
        personal.stats.u_turn_count,
        fast.stats.u_turn_count,
    );
    push_distance_change(
        &mut changes,
        "mi minor-road distance",
        personal.stats.minor_road_distance_m - fast.stats.minor_road_distance_m,
    );
    format!("Compared with the fastest route: {}.", changes.join(", "))
}

fn push_distance_change(changes: &mut Vec<String>, label: &str, delta_m: f64) {
    if delta_m.abs() > 1e-6 {
        changes.push(format!("{:+.1} {label}", miles(delta_m)));
    }
}

fn push_count_change(changes: &mut Vec<String>, label: &str, value: usize, baseline: usize) {
    let delta = value as i128 - baseline as i128;
    if delta != 0 {
        changes.push(format!("{delta:+} {label}"));
    }
}

fn position(coordinate: Coordinate) -> [f64; 2] {
    [coordinate.lon, coordinate.lat]
}

fn miles(meters: f64) -> f64 {
    meters / METERS_PER_MILE
}

fn coordinate_label(coordinate: Coordinate) -> String {
    format!("{:.5},{:.5}", coordinate.lat, coordinate.lon)
}

fn io_error(action: &'static str, path: &Path, source: std::io::Error) -> CliError {
    CliError::Io {
        action,
        path: PathBuf::from(path),
        source,
    }
}

#[cfg(test)]
mod tests {
    use myroute_core::{
        DrivingPreferences, Edge, EdgeId, Node, NodeId, RoadClass, RouteComparison,
    };

    use super::*;

    #[test]
    fn summary_contains_deltas_and_explanation() {
        let (graph, comparison) = fixture();
        let summary = terminal_summary(
            graph.nodes()[0].coordinate,
            graph.nodes()[3].coordinate,
            &comparison,
        );
        assert!(summary.contains("Difference:"));
        assert!(summary.contains("Route characteristics:"));
        assert!(summary.contains("Preference score:"));
        assert!(summary.contains("-1.2 mi highway distance"));
    }

    #[test]
    fn geojson_has_routes_and_four_endpoints() {
        let (graph, comparison) = fixture();
        let value = build_geojson(
            &graph,
            coordinate(0.0, 0.0),
            coordinate(0.0, 0.02),
            coordinate(0.0, 0.0),
            coordinate(0.0, 0.02),
            &comparison,
        )
        .unwrap();
        let features = value["features"].as_array().unwrap();
        assert_eq!(features.len(), 6);
        assert_eq!(features[0]["geometry"]["type"], "LineString");
        assert_eq!(features[1]["properties"]["kind"], "personalized");
    }

    #[test]
    fn empty_route_emits_point_at_snapped_node() {
        let (graph, _) = fixture();
        let empty = Route::analyze(&graph, Vec::new(), &DrivingPreferences::fastest()).unwrap();
        let comparison = RouteComparison::new(empty.clone(), empty);
        let point = coordinate(0.0, 0.0);
        let value = build_geojson(&graph, point, point, point, point, &comparison).unwrap();
        assert_eq!(value["features"][0]["geometry"]["type"], "Point");
        assert_eq!(
            value["features"][0]["geometry"]["coordinates"],
            json!([0.0, 0.0])
        );
        assert_eq!(value["features"][1]["geometry"]["type"], "Point");
    }

    #[test]
    fn html_embeds_data_safely_and_attributes_osm() {
        let malicious = json!({"value": "</script><script>alert(1)</script>&\u{2028}"});
        let html = html_document(&malicious).unwrap();
        assert!(!html.contains("</script><script>alert"));
        assert!(html.contains("\\u003c/script\\u003e"));
        assert!(html.contains("OpenStreetMap"));
        assert!(html.contains("L.geoJSON"));
        assert!(html.contains(".leaflet-tile-container"));
        assert!(html.contains("position: absolute"));
        assert!(!html.contains("rel=\"stylesheet\""));
    }

    fn fixture() -> (RoadGraph, RouteComparison) {
        let nodes = vec![
            Node::new(NodeId(0), coordinate(0.0, 0.0)),
            Node::new(NodeId(1), coordinate(0.0, 0.01)),
            Node::new(NodeId(2), coordinate(0.01, 0.01)),
            Node::new(NodeId(3), coordinate(0.0, 0.02)),
        ];
        let mut first = Edge::new(
            EdgeId(0),
            NodeId(0),
            NodeId(1),
            1_000.0,
            RoadClass::Motorway,
        );
        first.speed_limit_kph = Some(100.0);
        let mut second = Edge::new(
            EdgeId(1),
            NodeId(1),
            NodeId(3),
            1_000.0,
            RoadClass::Motorway,
        );
        second.speed_limit_kph = Some(100.0);
        let mut third = Edge::new(EdgeId(2), NodeId(0), NodeId(2), 1_200.0, RoadClass::Primary);
        third.speed_limit_kph = Some(90.0);
        let mut fourth = Edge::new(EdgeId(3), NodeId(2), NodeId(3), 1_200.0, RoadClass::Primary);
        fourth.speed_limit_kph = Some(90.0);
        let graph = RoadGraph::new(nodes, vec![first, second, third, fourth]).unwrap();
        let preferences = DrivingPreferences {
            highway_penalty: 1.0,
            ..DrivingPreferences::fastest()
        };
        let fastest = Route::analyze(&graph, vec![EdgeId(0), EdgeId(1)], &preferences).unwrap();
        let personal = Route::analyze(&graph, vec![EdgeId(2), EdgeId(3)], &preferences).unwrap();
        (graph, RouteComparison::new(fastest, personal))
    }

    fn coordinate(lat: f64, lon: f64) -> Coordinate {
        Coordinate::new(lat, lon).unwrap()
    }
}
