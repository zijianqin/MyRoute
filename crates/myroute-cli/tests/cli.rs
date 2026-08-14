use std::fs;
use std::path::Path;
use std::process::Command;

use myroute_core::{Coordinate, Edge, EdgeId, Node, NodeId, RoadClass, RoadGraph};
use osmpbfreader::{fileformat, osmformat};
use protobuf::{Message, MessageField};

#[test]
fn rejects_place_names_with_actionable_message() {
    let result = Command::new(env!("CARGO_BIN_EXE_myroute"))
        .args([
            "route",
            "--from",
            "Princeton, NJ",
            "--to",
            "40.4862,-74.4518",
        ])
        .output()
        .unwrap();
    assert!(!result.status.success());
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("geocoding is not supported"), "{stderr}");
}

#[test]
fn reports_a_missing_import_source_as_an_error() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("missing.osm.pbf");
    let result = Command::new(env!("CARGO_BIN_EXE_myroute"))
        .arg("import")
        .arg(&input)
        .output()
        .unwrap();
    assert!(!result.status.success());
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("failed to open OSM PBF"), "{stderr}");
    assert!(stderr.contains("missing.osm.pbf"), "{stderr}");
}

#[test]
fn imports_a_pbf_then_routes_the_serialized_graph() {
    let directory = tempfile::tempdir().unwrap();
    let pbf_path = directory.path().join("tiny.osm.pbf");
    let graph_path = directory.path().join("tiny.myroute");
    write_tiny_pbf(&pbf_path);

    let imported = Command::new(env!("CARGO_BIN_EXE_myroute"))
        .arg("import")
        .arg(&pbf_path)
        .args(["--output"])
        .arg(&graph_path)
        .output()
        .unwrap();
    assert!(
        imported.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&imported.stdout),
        String::from_utf8_lossy(&imported.stderr)
    );
    assert!(graph_path.is_file());
    assert!(String::from_utf8_lossy(&imported.stdout).contains("Created 2 directed edges"));

    let routed = Command::new(env!("CARGO_BIN_EXE_myroute"))
        .args([
            "route",
            "--from",
            "40.0000,-74.0000",
            "--to",
            "40.0010,-74.0000",
            "--graph",
        ])
        .arg(&graph_path)
        .output()
        .unwrap();
    assert!(
        routed.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&routed.stdout),
        String::from_utf8_lossy(&routed.stderr)
    );
    assert!(String::from_utf8_lossy(&routed.stdout).contains("Fastest route:"));
}

#[test]
fn routes_a_persisted_synthetic_graph_and_writes_map_files() {
    let directory = tempfile::tempdir().unwrap();
    let graph_path = directory.path().join("fixture.myroute");
    let output_path = directory.path().join("map");
    myroute_osm::save_graph(&graph_path, &graph()).unwrap();

    let result = Command::new(env!("CARGO_BIN_EXE_myroute"))
        .args([
            "route",
            "--from",
            "40.0000,-74.0000",
            "--to",
            "40.0010,-74.0000",
            "--graph",
        ])
        .arg(&graph_path)
        .args(["--profile", "chill", "--output"])
        .arg(&output_path)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(stdout.contains("Fastest route:"));
    assert!(stdout.contains("Personalized route:"));
    assert!(output_path.join("route.geojson").is_file());
    assert!(output_path.join("route.html").is_file());

    let geojson = std::fs::read_to_string(output_path.join("route.geojson")).unwrap();
    let value: serde_json::Value = serde_json::from_str(&geojson).unwrap();
    assert_eq!(value["type"], "FeatureCollection");
    assert_eq!(value["features"].as_array().unwrap().len(), 6);
}

#[test]
fn reports_a_missing_graph_as_an_error() {
    let directory = tempfile::tempdir().unwrap();
    let graph_path = directory.path().join("missing.myroute");
    let result = Command::new(env!("CARGO_BIN_EXE_myroute"))
        .args([
            "route",
            "--from",
            "40.0000,-74.0000",
            "--to",
            "40.0010,-74.0000",
            "--graph",
        ])
        .arg(&graph_path)
        .output()
        .unwrap();
    assert!(!result.status.success());
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("failed to read graph"), "{stderr}");
    assert!(stderr.contains("missing.myroute"), "{stderr}");
}

#[test]
#[ignore = "requires MYROUTE_NJ_GRAPH pointing to an imported New Jersey graph"]
fn new_jersey_feedback_route_avoids_the_three_reported_detours() {
    let graph = std::env::var_os("MYROUTE_NJ_GRAPH").expect("set MYROUTE_NJ_GRAPH");
    let directory = tempfile::tempdir().unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_myroute"))
        .args([
            "route",
            "--from",
            "40.3431,-74.6514",
            "--to",
            "40.4862,-74.4518",
            "--profile",
            "chill",
            "--graph",
        ])
        .arg(graph)
        .arg("--output")
        .arg(directory.path())
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    let geojson = fs::read_to_string(directory.path().join("route.geojson")).unwrap();
    let value: serde_json::Value = serde_json::from_str(&geojson).unwrap();
    let features = value["features"].as_array().unwrap();
    let fastest = features
        .iter()
        .find(|feature| feature["properties"]["kind"] == "fastest")
        .unwrap();
    let personalized = features
        .iter()
        .find(|feature| feature["properties"]["kind"] == "personalized")
        .unwrap();
    assert_eq!(personalized["properties"]["u_turn_count"], 0);
    assert_eq!(personalized["properties"]["signalized_left_turn_count"], 2);
    assert_eq!(personalized["geometry"], fastest["geometry"]);
}

fn graph() -> RoadGraph {
    let nodes = vec![
        Node::new(NodeId(0), Coordinate::new(40.0, -74.0).unwrap()),
        Node::new(NodeId(1), Coordinate::new(40.001, -74.0).unwrap()),
    ];
    let mut edge = Edge::new(EdgeId(0), NodeId(0), NodeId(1), 111.0, RoadClass::Primary);
    edge.speed_limit_kph = Some(40.0);
    RoadGraph::new(nodes, vec![edge]).unwrap()
}

fn write_tiny_pbf(path: &Path) {
    let mut strings = osmformat::StringTable::new();
    strings.s = vec![Vec::new(), b"highway".to_vec(), b"primary".to_vec()];

    let mut group = osmformat::PrimitiveGroup::new();
    group.nodes = vec![
        pbf_node(1, 400_000_000, -740_000_000),
        pbf_node(2, 400_010_000, -740_000_000),
    ];
    let mut way = osmformat::Way::new();
    way.id = Some(10);
    way.keys = vec![1];
    way.vals = vec![2];
    way.refs = vec![1, 1];
    group.ways.push(way);

    let mut block = osmformat::PrimitiveBlock::new();
    block.stringtable = MessageField::some(strings);
    block.primitivegroup.push(group);
    let mut blob = fileformat::Blob::new();
    blob.raw = Some(block.write_to_bytes().unwrap());
    let blob_bytes = blob.write_to_bytes().unwrap();
    let mut header = fileformat::BlobHeader::new();
    header.type_ = Some("OSMData".to_owned());
    header.datasize = Some(i32::try_from(blob_bytes.len()).unwrap());
    let header_bytes = header.write_to_bytes().unwrap();

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&u32::try_from(header_bytes.len()).unwrap().to_be_bytes());
    bytes.extend_from_slice(&header_bytes);
    bytes.extend_from_slice(&blob_bytes);
    fs::write(path, bytes).unwrap();
}

fn pbf_node(id: i64, lat: i64, lon: i64) -> osmformat::Node {
    let mut node = osmformat::Node::new();
    node.id = Some(id);
    node.lat = Some(lat);
    node.lon = Some(lon);
    node
}
