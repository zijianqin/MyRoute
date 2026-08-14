use std::fs;
use std::path::Path;

use osmpbfreader::{fileformat, osmformat};
use protobuf::{EnumOrUnknown, Message, MessageField};

#[test]
fn imports_an_actual_pbf_data_block() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tiny.osm.pbf");
    write_tiny_pbf(&path);

    let (graph, report) = myroute_osm::import_pbf(&path).unwrap();
    assert_eq!(report.parsed_nodes, 3);
    assert_eq!(report.parsed_ways, 1);
    assert_eq!(report.retained_ways, 1);
    assert_eq!(graph.node_count(), 3);
    assert_eq!(graph.edge_count(), 4);
    assert_eq!(
        graph
            .edges()
            .iter()
            .filter(|edge| edge.traffic_signal_at_end)
            .count(),
        2
    );
    assert!(graph.edges().iter().all(|edge| {
        edge.speed_limit_kph
            .is_some_and(|speed| (speed - 40.233_6).abs() < 1e-9)
    }));
}

#[test]
fn imports_a_via_node_turn_restriction_from_pbf() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("restriction.osm.pbf");
    write_restriction_pbf(&path);

    let (graph, report) = myroute_osm::import_pbf(&path).unwrap();
    assert_eq!(report.parsed_relations, 1);
    assert_eq!(report.restriction_relations, 1);
    assert_eq!(report.applied_restriction_relations, 1);
    assert_eq!(report.created_turn_restrictions, 1);
    let from = graph
        .edges()
        .iter()
        .find(|edge| edge.source_way_id == Some(10) && edge.to.0 == 1)
        .unwrap();
    let to = graph
        .edges()
        .iter()
        .find(|edge| edge.source_way_id == Some(20) && edge.from.0 == 1)
        .unwrap();
    assert!(!graph.transition_allowed(from.id, to.id).unwrap());
}

fn write_tiny_pbf(path: &Path) {
    let mut strings = osmformat::StringTable::new();
    strings.s = vec![
        Vec::new(),
        b"highway".to_vec(),
        b"primary".to_vec(),
        b"traffic_signals".to_vec(),
        b"maxspeed".to_vec(),
        b"25 mph".to_vec(),
    ];

    let mut group = osmformat::PrimitiveGroup::new();
    group.nodes = vec![
        node(1, 400_000_000, -740_000_000, None),
        node(2, 400_010_000, -740_000_000, Some((1, 3))),
        node(3, 400_020_000, -740_000_000, None),
    ];
    let mut way = osmformat::Way::new();
    way.id = Some(10);
    way.keys = vec![1, 4];
    way.vals = vec![2, 5];
    way.refs = vec![1, 1, 1];
    group.ways.push(way);

    let mut block = osmformat::PrimitiveBlock::new();
    block.stringtable = MessageField::some(strings);
    block.primitivegroup.push(group);
    let block_bytes = block.write_to_bytes().unwrap();

    let mut blob = fileformat::Blob::new();
    blob.raw = Some(block_bytes);
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

fn write_restriction_pbf(path: &Path) {
    let mut strings = osmformat::StringTable::new();
    strings.s = vec![
        Vec::new(),
        b"highway".to_vec(),
        b"primary".to_vec(),
        b"type".to_vec(),
        b"restriction".to_vec(),
        b"no_straight_on".to_vec(),
        b"from".to_vec(),
        b"via".to_vec(),
        b"to".to_vec(),
    ];

    let mut group = osmformat::PrimitiveGroup::new();
    group.nodes = vec![
        node(1, 399_990_000, -740_000_000, None),
        node(2, 400_000_000, -740_000_000, None),
        node(3, 400_010_000, -740_000_000, None),
        node(4, 400_000_000, -740_010_000, None),
    ];
    group.ways = vec![
        pbf_way(10, &[1, 1]),
        pbf_way(20, &[2, 1]),
        pbf_way(30, &[2, 2]),
    ];
    let mut relation = osmformat::Relation::new();
    relation.id = Some(100);
    relation.keys = vec![3, 4];
    relation.vals = vec![4, 5];
    relation.roles_sid = vec![6, 7, 8];
    relation.memids = vec![10, -8, 18];
    relation.types = vec![
        EnumOrUnknown::new(osmformat::relation::MemberType::WAY),
        EnumOrUnknown::new(osmformat::relation::MemberType::NODE),
        EnumOrUnknown::new(osmformat::relation::MemberType::WAY),
    ];
    group.relations.push(relation);

    write_block(path, strings, group);
}

fn pbf_way(id: i64, refs: &[i64]) -> osmformat::Way {
    let mut way = osmformat::Way::new();
    way.id = Some(id);
    way.keys = vec![1];
    way.vals = vec![2];
    way.refs = refs.to_vec();
    way
}

fn write_block(path: &Path, strings: osmformat::StringTable, group: osmformat::PrimitiveGroup) {
    let mut block = osmformat::PrimitiveBlock::new();
    block.stringtable = MessageField::some(strings);
    block.primitivegroup.push(group);
    let block_bytes = block.write_to_bytes().unwrap();

    let mut blob = fileformat::Blob::new();
    blob.raw = Some(block_bytes);
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

fn node(id: i64, lat: i64, lon: i64, tag: Option<(u32, u32)>) -> osmformat::Node {
    let mut node = osmformat::Node::new();
    node.id = Some(id);
    node.lat = Some(lat);
    node.lon = Some(lon);
    if let Some((key, value)) = tag {
        node.keys.push(key);
        node.vals.push(value);
    }
    node
}
