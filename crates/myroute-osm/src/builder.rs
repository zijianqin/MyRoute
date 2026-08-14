use std::collections::{BTreeMap, BTreeSet, VecDeque};

use myroute_core::{
    Coordinate, Edge, EdgeId, Node, NodeId, RoadClass, RoadGraph, TurnRestriction,
    haversine_distance_m,
};
use osmpbfreader::{
    Node as OsmNode, NodeId as OsmNodeId, OsmId, OsmObj, Relation, Tags, Way, WayId,
};

use crate::parser::ImportReport;
use crate::{Error, Result};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Direction {
    Both,
    Forward,
    Reverse,
}

const MAX_SIGNAL_CONTROL_PROPAGATION_M: f64 = 50.0;

struct Segment<'a> {
    way: &'a Way,
    from: OsmNodeId,
    to: OsmNodeId,
    distance_m: f64,
    shape: Vec<Coordinate>,
}

pub(crate) fn is_drivable_way(way: &Way) -> bool {
    road_class(&way.tags).is_some() && !access_denied(&way.tags)
}

pub(crate) fn is_turn_restriction_relation(relation: &Relation) -> bool {
    tag(&relation.tags, "type").is_some_and(|value| value.eq_ignore_ascii_case("restriction"))
}

pub(crate) fn build_graph(
    objects: &BTreeMap<OsmId, OsmObj>,
    report: &mut ImportReport,
) -> Result<RoadGraph> {
    let ways: Vec<_> = objects
        .values()
        .filter_map(OsmObj::way)
        .filter(|way| is_drivable_way(way))
        .collect();
    let mut reference_counts = BTreeMap::<OsmNodeId, usize>::new();
    for way in ways.iter().filter(|way| way.nodes.len() >= 2) {
        for &node_id in &way.nodes {
            osm_node(objects, way.id.0, node_id)?;
        }
        let unique_nodes: BTreeSet<_> = way.nodes.iter().copied().collect();
        for node_id in unique_nodes {
            *reference_counts.entry(node_id).or_default() += 1;
        }
    }

    let mut segments = Vec::new();
    for way in ways {
        if way.nodes.len() < 2 {
            report.invalid_ways += 1;
            continue;
        }
        let way_segments = segments_for_way(way, objects, &reference_counts)?;
        if way_segments.is_empty() {
            report.invalid_ways += 1;
        } else {
            report.retained_ways += 1;
            segments.extend(way_segments);
        }
    }

    let endpoint_ids: BTreeSet<_> = segments
        .iter()
        .flat_map(|segment| [segment.from, segment.to])
        .collect();
    check_capacity("nodes", endpoint_ids.len())?;
    let mut node_ids = BTreeMap::new();
    let mut nodes = Vec::with_capacity(endpoint_ids.len());
    for (index, osm_id) in endpoint_ids.into_iter().enumerate() {
        let id = NodeId(u32::try_from(index).map_err(|_| Error::IdCapacity {
            kind: "nodes",
            count: index + 1,
        })?);
        let coordinate = coordinate(osm_node(objects, 0, osm_id)?)?;
        node_ids.insert(osm_id, id);
        nodes.push(Node::new(id, coordinate));
    }

    let expected_edges = segments.iter().try_fold(0usize, |count, segment| {
        count
            .checked_add(match direction(&segment.way.tags) {
                Direction::Both => 2,
                Direction::Forward | Direction::Reverse => 1,
            })
            .ok_or(Error::IdCapacity {
                kind: "edges",
                count: usize::MAX,
            })
    })?;
    check_capacity("edges", expected_edges)?;
    let mut edges = Vec::with_capacity(expected_edges);
    for segment in segments {
        let attributes = WayAttributes {
            road_class: road_class(&segment.way.tags).expect("retained ways have a road class"),
            speed_limit_kph: tag(&segment.way.tags, "maxspeed").and_then(parse_maxspeed),
            lanes: tag(&segment.way.tags, "lanes").and_then(parse_lanes),
            name: tag(&segment.way.tags, "name")
                .filter(|name| !name.trim().is_empty())
                .map(str::to_owned),
        };
        match direction(&segment.way.tags) {
            Direction::Both => {
                push_edge(&mut edges, &node_ids, objects, &segment, &attributes, false)?;
                push_edge(&mut edges, &node_ids, objects, &segment, &attributes, true)?;
            }
            Direction::Forward => {
                push_edge(&mut edges, &node_ids, objects, &segment, &attributes, false)?
            }
            Direction::Reverse => {
                push_edge(&mut edges, &node_ids, objects, &segment, &attributes, true)?
            }
        }
    }
    propagate_signal_control(&mut edges);

    report.retained_nodes = nodes.len();
    report.created_edges = edges.len();
    let restrictions = build_turn_restrictions(objects, &node_ids, &edges, report);
    report.created_turn_restrictions = restrictions.len();
    RoadGraph::with_turn_restrictions(nodes, edges, restrictions).map_err(Error::from)
}

fn segments_for_way<'a>(
    way: &'a Way,
    objects: &BTreeMap<OsmId, OsmObj>,
    reference_counts: &BTreeMap<OsmNodeId, usize>,
) -> Result<Vec<Segment<'a>>> {
    if way.is_closed() {
        closed_segments(way, objects, reference_counts)
    } else {
        open_segments(way, objects, reference_counts)
    }
}

fn open_segments<'a>(
    way: &'a Way,
    objects: &BTreeMap<OsmId, OsmObj>,
    reference_counts: &BTreeMap<OsmNodeId, usize>,
) -> Result<Vec<Segment<'a>>> {
    let mut boundaries = vec![0];
    for index in 1..way.nodes.len() - 1 {
        if is_routing_node(way.nodes[index], objects, reference_counts)? {
            boundaries.push(index);
        }
    }
    boundaries.push(way.nodes.len() - 1);
    boundaries
        .windows(2)
        .filter_map(|pair| make_segment(way, &way.nodes[pair[0]..=pair[1]], objects).transpose())
        .collect()
}

fn closed_segments<'a>(
    way: &'a Way,
    objects: &BTreeMap<OsmId, OsmObj>,
    reference_counts: &BTreeMap<OsmNodeId, usize>,
) -> Result<Vec<Segment<'a>>> {
    let ring = &way.nodes[..way.nodes.len() - 1];
    if ring.is_empty() {
        return Ok(Vec::new());
    }
    let mut boundaries = Vec::new();
    let mut boundary_ids = BTreeSet::new();
    for (index, &node_id) in ring.iter().enumerate() {
        if is_routing_node(node_id, objects, reference_counts)? && boundary_ids.insert(node_id) {
            boundaries.push(index);
        }
    }
    if boundaries.is_empty() {
        boundaries.push(0);
    }
    if boundaries.len() == 1
        && let Some(index) = farthest_distinct_index(ring, boundaries[0])
    {
        boundaries.push(index);
        boundaries.sort_unstable();
    }
    if boundaries.len() < 2 {
        return Ok(Vec::new());
    }

    let mut segments = Vec::new();
    for (position, &start) in boundaries.iter().enumerate() {
        let end = boundaries[(position + 1) % boundaries.len()];
        let mut node_ids = vec![ring[start]];
        let mut index = (start + 1) % ring.len();
        loop {
            node_ids.push(ring[index]);
            if index == end {
                break;
            }
            index = (index + 1) % ring.len();
        }
        if let Some(segment) = make_segment(way, &node_ids, objects)? {
            segments.push(segment);
        }
    }
    Ok(segments)
}

fn farthest_distinct_index(ring: &[OsmNodeId], anchor: usize) -> Option<usize> {
    let mut best = None;
    for (index, &node_id) in ring.iter().enumerate() {
        if node_id == ring[anchor] {
            continue;
        }
        let clockwise = index.abs_diff(anchor);
        let separation = clockwise.min(ring.len() - clockwise);
        if best.is_none_or(|(_, best_separation)| separation > best_separation) {
            best = Some((index, separation));
        }
    }
    best.map(|(index, _)| index)
}

fn make_segment<'a>(
    way: &'a Way,
    node_ids: &[OsmNodeId],
    objects: &BTreeMap<OsmId, OsmObj>,
) -> Result<Option<Segment<'a>>> {
    let Some((&from, &to)) = node_ids.first().zip(node_ids.last()) else {
        return Ok(None);
    };
    if from == to {
        return Ok(None);
    }
    let shape: Vec<_> = node_ids
        .iter()
        .map(|&node_id| coordinate(osm_node(objects, way.id.0, node_id)?))
        .collect::<Result<_>>()?;
    let distance_m = shape
        .windows(2)
        .map(|pair| haversine_distance_m(pair[0], pair[1]))
        .sum::<f64>();
    if !distance_m.is_finite() || distance_m <= 0.0 {
        return Ok(None);
    }
    Ok(Some(Segment {
        way,
        from,
        to,
        distance_m,
        shape,
    }))
}

struct WayAttributes {
    road_class: RoadClass,
    speed_limit_kph: Option<f64>,
    lanes: Option<u8>,
    name: Option<String>,
}

fn push_edge(
    edges: &mut Vec<Edge>,
    node_ids: &BTreeMap<OsmNodeId, NodeId>,
    objects: &BTreeMap<OsmId, OsmObj>,
    segment: &Segment<'_>,
    attributes: &WayAttributes,
    reverse: bool,
) -> Result<()> {
    let (from_osm, to_osm) = if reverse {
        (segment.to, segment.from)
    } else {
        (segment.from, segment.to)
    };
    let mut shape = segment.shape.clone();
    if reverse {
        shape.reverse();
    }
    let id = EdgeId(u32::try_from(edges.len()).map_err(|_| Error::IdCapacity {
        kind: "edges",
        count: edges.len() + 1,
    })?);
    let mut edge = Edge::new(
        id,
        node_ids[&from_osm],
        node_ids[&to_osm],
        segment.distance_m,
        attributes.road_class,
    )
    .with_shape(shape);
    edge.speed_limit_kph = attributes.speed_limit_kph;
    edge.lanes = attributes.lanes;
    edge.name.clone_from(&attributes.name);
    edge.source_way_id = Some(segment.way.id.0);
    edge.traffic_signal_at_end =
        traffic_signal_applies(osm_node(objects, segment.way.id.0, to_osm)?, reverse);
    edge.signal_controlled_at_end = edge.traffic_signal_at_end;
    edges.push(edge);
    Ok(())
}

fn propagate_signal_control(edges: &mut [Edge]) {
    let mut outgoing = BTreeMap::<NodeId, Vec<EdgeId>>::new();
    for edge in edges.iter() {
        outgoing.entry(edge.from).or_default().push(edge.id);
    }
    let seeds: Vec<_> = edges
        .iter()
        .filter(|edge| edge.traffic_signal_at_end)
        .map(|edge| edge.id)
        .collect();
    for seed in seeds {
        let mut queue = VecDeque::from([(seed, 0.0)]);
        let mut visited = BTreeSet::from([seed]);
        while let Some((previous, distance_m)) = queue.pop_front() {
            let previous_edge = &edges[previous.index()];
            let previous_from = previous_edge.from;
            let previous_to = previous_edge.to;
            let previous_source_way_id = previous_edge.source_way_id;
            let previous_name = previous_edge.name.clone();
            let candidates: Vec<_> = outgoing
                .get(&previous_to)
                .into_iter()
                .flatten()
                .copied()
                .filter(|&next| edges[next.index()].to != previous_from)
                .collect();
            for next in candidates {
                let next_edge = &edges[next.index()];
                let same_name = previous_name
                    .as_ref()
                    .zip(next_edge.name.as_ref())
                    .is_some_and(|(previous, next)| previous == next);
                if previous_source_way_id != next_edge.source_way_id && !same_name {
                    continue;
                }
                let next_distance_m = distance_m + next_edge.distance_m;
                if next_distance_m > MAX_SIGNAL_CONTROL_PROPAGATION_M || !visited.insert(next) {
                    continue;
                }
                edges[next.index()].signal_controlled_at_end = true;
                queue.push_back((next, next_distance_m));
            }
        }
    }
}

fn is_routing_node(
    node_id: OsmNodeId,
    objects: &BTreeMap<OsmId, OsmObj>,
    reference_counts: &BTreeMap<OsmNodeId, usize>,
) -> Result<bool> {
    Ok(reference_counts.get(&node_id).copied().unwrap_or(0) > 1
        || is_traffic_signal(osm_node(objects, 0, node_id)?))
}

fn osm_node(
    objects: &BTreeMap<OsmId, OsmObj>,
    way_id: i64,
    node_id: OsmNodeId,
) -> Result<&OsmNode> {
    objects
        .get(&OsmId::Node(node_id))
        .and_then(OsmObj::node)
        .ok_or(Error::MissingNodeDependency {
            way_id,
            node_id: node_id.0,
        })
}

fn coordinate(node: &OsmNode) -> Result<Coordinate> {
    Coordinate::new(node.lat(), node.lon()).map_err(Error::from)
}

fn is_traffic_signal(node: &OsmNode) -> bool {
    tag(&node.tags, "highway").is_some_and(|value| value.eq_ignore_ascii_case("traffic_signals"))
}

fn traffic_signal_applies(node: &OsmNode, reverse: bool) -> bool {
    if !is_traffic_signal(node) {
        return false;
    }
    match tag(&node.tags, "traffic_signals:direction").map(str::trim) {
        Some(value) if value.eq_ignore_ascii_case("forward") => !reverse,
        Some(value) if value.eq_ignore_ascii_case("backward") => reverse,
        _ => true,
    }
}

#[derive(Clone, Copy)]
enum RestrictionMode {
    No,
    Only,
}

struct ParsedRestriction {
    mode: RestrictionMode,
    from: WayId,
    via: OsmNodeId,
    to: WayId,
    same_way_u_turn: bool,
}

enum RestrictionParse {
    Apply(ParsedRestriction),
    Exempt,
    Unsupported,
}

fn build_turn_restrictions(
    objects: &BTreeMap<OsmId, OsmObj>,
    node_ids: &BTreeMap<OsmNodeId, NodeId>,
    edges: &[Edge],
    report: &mut ImportReport,
) -> Vec<TurnRestriction> {
    let mut restrictions = BTreeSet::new();
    let mut incoming_by_way_node = BTreeMap::<(i64, NodeId), Vec<EdgeId>>::new();
    let mut outgoing_by_node = BTreeMap::<NodeId, Vec<EdgeId>>::new();
    for edge in edges {
        if let Some(way_id) = edge.source_way_id {
            incoming_by_way_node
                .entry((way_id, edge.to))
                .or_default()
                .push(edge.id);
        }
        outgoing_by_node.entry(edge.from).or_default().push(edge.id);
    }
    for relation in objects
        .values()
        .filter_map(OsmObj::relation)
        .filter(|relation| is_turn_restriction_relation(relation))
    {
        let parsed = match parse_turn_restriction(relation) {
            RestrictionParse::Apply(parsed) => parsed,
            RestrictionParse::Exempt => {
                report.exempt_restriction_relations += 1;
                continue;
            }
            RestrictionParse::Unsupported => {
                report.unsupported_restriction_relations += 1;
                continue;
            }
        };
        let Some(&via) = node_ids.get(&parsed.via) else {
            report.unsupported_restriction_relations += 1;
            continue;
        };
        let incoming = incoming_by_way_node
            .get(&(parsed.from.0, via))
            .map(Vec::as_slice)
            .unwrap_or_default();
        let outgoing = outgoing_by_node
            .get(&via)
            .map(Vec::as_slice)
            .unwrap_or_default();
        if incoming.is_empty() || outgoing.is_empty() {
            report.unsupported_restriction_relations += 1;
            continue;
        }
        report.applied_restriction_relations += 1;
        for &previous_id in incoming {
            let previous = &edges[previous_id.index()];
            for &next_id in outgoing {
                let next = &edges[next_id.index()];
                let prohibited = match parsed.mode {
                    RestrictionMode::No if parsed.same_way_u_turn => {
                        next.source_way_id == Some(parsed.to.0) && previous.from == next.to
                    }
                    RestrictionMode::No => next.source_way_id == Some(parsed.to.0),
                    RestrictionMode::Only => next.source_way_id != Some(parsed.to.0),
                };
                if prohibited {
                    restrictions.insert(TurnRestriction::new(previous.id, next.id));
                }
            }
        }
    }
    restrictions.into_iter().collect()
}

fn parse_turn_restriction(relation: &Relation) -> RestrictionParse {
    if tag(&relation.tags, "except").is_some_and(motorcar_is_exempt) {
        return RestrictionParse::Exempt;
    }
    let value = [
        "restriction:motorcar",
        "restriction:motor_vehicle",
        "restriction",
    ]
    .into_iter()
    .find_map(|key| tag(&relation.tags, key));
    let Some(value) = value else {
        return RestrictionParse::Unsupported;
    };
    let normalized = value.trim().to_ascii_lowercase();
    let mode = if normalized.starts_with("no_") {
        RestrictionMode::No
    } else if normalized.starts_with("only_") {
        RestrictionMode::Only
    } else {
        return RestrictionParse::Unsupported;
    };
    let from: Vec<_> = relation
        .refs
        .iter()
        .filter(|reference| reference.role.eq_ignore_ascii_case("from"))
        .filter_map(|reference| reference.member.way())
        .collect();
    let via: Vec<_> = relation
        .refs
        .iter()
        .filter(|reference| reference.role.eq_ignore_ascii_case("via"))
        .filter_map(|reference| reference.member.node())
        .collect();
    let to: Vec<_> = relation
        .refs
        .iter()
        .filter(|reference| reference.role.eq_ignore_ascii_case("to"))
        .filter_map(|reference| reference.member.way())
        .collect();
    if from.len() != 1 || via.len() != 1 || to.len() != 1 {
        return RestrictionParse::Unsupported;
    }
    let from = from[0];
    let via = via[0];
    let to = to[0];
    RestrictionParse::Apply(ParsedRestriction {
        mode,
        from,
        via,
        to,
        same_way_u_turn: normalized == "no_u_turn" && from == to,
    })
}

fn motorcar_is_exempt(value: &str) -> bool {
    value
        .split([';', ','])
        .map(str::trim)
        .any(|mode| matches!(mode, "motorcar" | "motor_vehicle" | "vehicle"))
}

fn road_class(tags: &Tags) -> Option<RoadClass> {
    let highway = tag(tags, "highway")?.to_ascii_lowercase();
    match highway.as_str() {
        "motorway" | "motorway_link" => Some(RoadClass::Motorway),
        "trunk" | "trunk_link" => Some(RoadClass::Trunk),
        "primary" | "primary_link" => Some(RoadClass::Primary),
        "secondary" | "secondary_link" => Some(RoadClass::Secondary),
        "tertiary" | "tertiary_link" => Some(RoadClass::Tertiary),
        "residential" => Some(RoadClass::Residential),
        "service" => Some(RoadClass::Service),
        "living_street" | "road" | "unclassified" => Some(RoadClass::Other),
        _ => None,
    }
}

fn access_denied(tags: &Tags) -> bool {
    ["motorcar", "motor_vehicle", "vehicle", "access"]
        .into_iter()
        .find_map(|key| tag(tags, key))
        .is_some_and(|value| {
            value.eq_ignore_ascii_case("no") || value.eq_ignore_ascii_case("private")
        })
}

fn direction(tags: &Tags) -> Direction {
    match tag(tags, "oneway").map(str::trim) {
        Some(value)
            if value.eq_ignore_ascii_case("yes")
                || value == "1"
                || value.eq_ignore_ascii_case("true") =>
        {
            Direction::Forward
        }
        Some(value) if value == "-1" || value.eq_ignore_ascii_case("reverse") => Direction::Reverse,
        Some(value)
            if value.eq_ignore_ascii_case("no")
                || value == "0"
                || value.eq_ignore_ascii_case("false") =>
        {
            Direction::Both
        }
        _ if tag(tags, "junction")
            .is_some_and(|value| value.eq_ignore_ascii_case("roundabout")) =>
        {
            Direction::Forward
        }
        _ => Direction::Both,
    }
}

fn parse_maxspeed(value: &str) -> Option<f64> {
    let value = value.trim().to_ascii_lowercase();
    let (number, factor) = if let Some(number) = value.strip_suffix("mph") {
        (number, 1.609_344)
    } else if let Some(number) = value.strip_suffix("km/h") {
        (number, 1.0)
    } else if let Some(number) = value.strip_suffix("kmh") {
        (number, 1.0)
    } else if let Some(number) = value.strip_suffix("kph") {
        (number, 1.0)
    } else {
        (value.as_str(), 1.0)
    };
    let speed = number.trim().parse::<f64>().ok()? * factor;
    (speed.is_finite() && speed > 0.0).then_some(speed)
}

fn parse_lanes(value: &str) -> Option<u8> {
    let lanes = value.trim().parse::<u8>().ok()?;
    (lanes > 0).then_some(lanes)
}

fn tag<'a>(tags: &'a Tags, key: &str) -> Option<&'a str> {
    tags.get(key).map(|value| value.as_str())
}

fn check_capacity(kind: &'static str, count: usize) -> Result<()> {
    if count > u32::MAX as usize + 1 {
        Err(Error::IdCapacity { kind, count })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use myroute_core::{DijkstraRouter, DrivingPreferences, RouteRequest};
    use osmpbfreader::{NodeId as OsmNodeId, Ref, RelationId, WayId};

    #[test]
    fn tag_helpers_classify_and_filter_roads() {
        assert_eq!(
            road_class(&tags(&[("highway", "motorway_link")])),
            Some(RoadClass::Motorway)
        );
        assert_eq!(
            road_class(&tags(&[("highway", "living_street")])),
            Some(RoadClass::Other)
        );
        assert_eq!(road_class(&tags(&[("highway", "footway")])), None);

        let specific_override = tags(&[
            ("highway", "service"),
            ("access", "private"),
            ("motorcar", "yes"),
        ]);
        assert!(!access_denied(&specific_override));
        assert!(is_drivable_way(&way(10, &[1, 2], specific_override)));
        assert!(!is_drivable_way(&way(
            11,
            &[1, 2],
            tags(&[("highway", "primary"), ("motor_vehicle", "no")]),
        )));
    }

    #[test]
    fn parses_speed_and_lanes_conservatively() {
        assert_eq!(parse_maxspeed("50"), Some(50.0));
        assert_eq!(parse_maxspeed("80 km/h"), Some(80.0));
        assert!((parse_maxspeed("25 mph").unwrap() - 40.233_6).abs() < 1e-9);
        assert_eq!(parse_maxspeed("signals"), None);
        assert_eq!(parse_maxspeed("0"), None);
        assert_eq!(parse_lanes("3"), Some(3));
        assert_eq!(parse_lanes("2;1"), None);
        assert_eq!(parse_lanes("0"), None);
    }

    #[test]
    fn interprets_oneway_and_roundabout_direction() {
        assert_eq!(direction(&tags(&[("oneway", "yes")])), Direction::Forward);
        assert_eq!(direction(&tags(&[("oneway", "-1")])), Direction::Reverse);
        assert_eq!(
            direction(&tags(&[("junction", "roundabout")])),
            Direction::Forward
        );
        assert_eq!(
            direction(&tags(&[("junction", "roundabout"), ("oneway", "no")])),
            Direction::Both
        );
    }

    #[test]
    fn keeps_curve_points_in_bidirectional_edge_shapes() {
        let objects = objects(
            &[
                (1, 40.0, -74.0, false),
                (2, 40.001, -73.999, false),
                (3, 40.002, -74.0, false),
            ],
            vec![way(
                10,
                &[1, 2, 3],
                tags(&[
                    ("highway", "primary"),
                    ("maxspeed", "25 mph"),
                    ("lanes", "2"),
                    ("name", "Arc Road"),
                ]),
            )],
        );
        let graph = build(&objects);
        assert_eq!(graph.node_count(), 2);
        assert_eq!(graph.edge_count(), 2);
        assert_eq!(graph.edges()[0].shape.len(), 3);
        assert_eq!(
            graph.edges()[1].shape,
            graph.edges()[0]
                .shape
                .iter()
                .rev()
                .copied()
                .collect::<Vec<_>>()
        );
        assert_eq!(graph.edges()[0].name.as_deref(), Some("Arc Road"));
        assert_eq!(graph.edges()[0].lanes, Some(2));
        assert_eq!(graph.edges()[0].source_way_id, Some(10));
    }

    #[test]
    fn reverse_oneway_reverses_endpoints_and_shape() {
        let objects = objects(
            &[
                (1, 40.0, -74.0, false),
                (2, 40.001, -74.0, false),
                (3, 40.002, -74.0, false),
            ],
            vec![way(
                10,
                &[1, 2, 3],
                tags(&[("highway", "primary"), ("oneway", "-1")]),
            )],
        );
        let graph = build(&objects);
        let edge = &graph.edges()[0];
        assert_eq!((edge.from, edge.to), (NodeId(1), NodeId(0)));
        assert!((edge.shape[0].lat - 40.002).abs() < 1e-10);
        assert_eq!(edge.shape[2].lat, 40.0);
    }

    #[test]
    fn signal_node_splits_way_and_marks_incoming_edges() {
        let objects = objects(
            &[
                (1, 40.0, -74.0, false),
                (2, 40.001, -74.0, true),
                (3, 40.002, -74.0, false),
            ],
            vec![way(10, &[1, 2, 3], tags(&[("highway", "secondary")]))],
        );
        let graph = build(&objects);
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
        assert!(graph.edges()[0].traffic_signal_at_end);
        assert!(graph.edges()[3].traffic_signal_at_end);
    }

    #[test]
    fn directional_signal_marks_only_the_matching_approach() {
        for (direction, expected_edge) in [("forward", 0), ("backward", 3)] {
            let mut objects = objects(
                &[
                    (1, 40.0, -74.0, false),
                    (2, 40.001, -74.0, true),
                    (3, 40.002, -74.0, false),
                ],
                vec![way(10, &[1, 2, 3], tags(&[("highway", "secondary")]))],
            );
            let OsmObj::Node(signal) = objects.get_mut(&OsmId::Node(OsmNodeId(2))).unwrap() else {
                panic!("expected signal node")
            };
            signal
                .tags
                .insert("traffic_signals:direction".into(), direction.into());
            let graph = build(&objects);
            let signaled: Vec<_> = graph
                .edges()
                .iter()
                .filter(|edge| edge.traffic_signal_at_end)
                .map(|edge| edge.id.0)
                .collect();
            assert_eq!(signaled, vec![expected_edge]);
        }
    }

    #[test]
    fn signal_control_propagates_to_a_nearby_intersection_without_duplicate_signal() {
        let objects = objects(
            &[
                (1, 40.0, -74.0, false),
                (2, 40.0001, -74.0, true),
                (3, 40.0002, -74.0, false),
                (4, 40.0002, -74.0001, false),
            ],
            vec![
                way(10, &[1, 2, 3], tags(&[("highway", "secondary")])),
                way(20, &[3, 4], tags(&[("highway", "secondary")])),
            ],
        );
        let graph = build(&objects);
        let physical_signals: Vec<_> = graph
            .edges()
            .iter()
            .filter(|edge| edge.traffic_signal_at_end)
            .map(|edge| edge.id)
            .collect();
        assert_eq!(physical_signals.len(), 2);
        let approaching_intersection = graph
            .edges()
            .iter()
            .find(|edge| edge.source_way_id == Some(10) && edge.from.0 == 1 && edge.to.0 == 2)
            .unwrap();
        assert!(!approaching_intersection.traffic_signal_at_end);
        assert!(approaching_intersection.signal_controlled_at_end);
    }

    #[test]
    fn imports_no_and_only_via_node_restrictions() {
        let nodes = &[
            (1, 39.999, -74.0, false),
            (2, 40.0, -74.0, false),
            (3, 40.001, -74.0, false),
            (4, 40.0, -74.001, false),
        ];
        let ways = vec![
            way(10, &[1, 2], tags(&[("highway", "primary")])),
            way(20, &[2, 3], tags(&[("highway", "primary")])),
            way(30, &[2, 4], tags(&[("highway", "primary")])),
        ];

        let mut no_objects = objects(nodes, ways.clone());
        insert_relation(
            &mut no_objects,
            relation(
                100,
                "no_straight_on",
                OsmId::Node(OsmNodeId(2)),
                Tags::new(),
            ),
        );
        let mut report = ImportReport::default();
        let no_graph = build_graph(&no_objects, &mut report).unwrap();
        assert_eq!(report.applied_restriction_relations, 1);
        assert_eq!(report.created_turn_restrictions, 1);
        assert!(!no_graph.transition_allowed(EdgeId(0), EdgeId(2)).unwrap());
        assert!(no_graph.transition_allowed(EdgeId(0), EdgeId(4)).unwrap());

        let mut only_objects = objects(nodes, ways);
        insert_relation(
            &mut only_objects,
            relation(
                101,
                "only_straight_on",
                OsmId::Node(OsmNodeId(2)),
                Tags::new(),
            ),
        );
        let only_graph = build(&only_objects);
        assert!(only_graph.transition_allowed(EdgeId(0), EdgeId(2)).unwrap());
        assert!(!only_graph.transition_allowed(EdgeId(0), EdgeId(4)).unwrap());
    }

    #[test]
    fn reports_exempt_and_unsupported_restrictions() {
        let nodes = &[
            (1, 39.999, -74.0, false),
            (2, 40.0, -74.0, false),
            (3, 40.001, -74.0, false),
        ];
        let ways = vec![
            way(10, &[1, 2], tags(&[("highway", "primary")])),
            way(20, &[2, 3], tags(&[("highway", "primary")])),
        ];
        let mut map = objects(nodes, ways);
        insert_relation(
            &mut map,
            relation(
                100,
                "no_straight_on",
                OsmId::Node(OsmNodeId(2)),
                tags(&[("except", "motorcar")]),
            ),
        );
        insert_relation(
            &mut map,
            relation(101, "no_straight_on", OsmId::Way(WayId(30)), Tags::new()),
        );
        let mut report = ImportReport::default();
        let graph = build_graph(&map, &mut report).unwrap();
        assert_eq!(report.exempt_restriction_relations, 1);
        assert_eq!(report.unsupported_restriction_relations, 1);
        assert!(graph.turn_restrictions().is_empty());
    }

    #[test]
    fn shared_node_splits_retained_ways() {
        let objects = objects(
            &[
                (1, 40.0, -74.0, false),
                (2, 40.001, -74.0, false),
                (3, 40.002, -74.0, false),
                (4, 40.001, -73.999, false),
            ],
            vec![
                way(10, &[1, 2, 3], tags(&[("highway", "primary")])),
                way(20, &[2, 4], tags(&[("highway", "residential")])),
            ],
        );
        let graph = build(&objects);
        assert_eq!(graph.node_count(), 4);
        assert_eq!(graph.edge_count(), 6);
        assert!(graph.edges().iter().all(|edge| edge.shape.len() == 2));
    }

    #[test]
    fn closed_way_without_natural_boundaries_uses_synthetic_vertices() {
        let objects = objects(
            &[
                (1, 40.0, -74.0, false),
                (2, 40.001, -73.999, false),
                (3, 40.0, -73.998, false),
            ],
            vec![way(10, &[1, 2, 3, 1], tags(&[("highway", "primary")]))],
        );
        let graph = build(&objects);
        assert_eq!(graph.node_count(), 2);
        assert_eq!(graph.edge_count(), 4);
        assert_no_self_loops(&graph);
        assert_route(&graph, NodeId(0), NodeId(1));
        assert_route(&graph, NodeId(1), NodeId(0));
    }

    #[test]
    fn closed_way_with_one_natural_boundary_remains_connected() {
        let objects = objects(
            &[
                (1, 40.0, -74.0, false),
                (2, 40.001, -73.999, false),
                (3, 40.0, -73.998, false),
                (4, 39.999, -74.0, false),
            ],
            vec![
                way(10, &[1, 2, 3, 1], tags(&[("highway", "primary")])),
                way(20, &[1, 4], tags(&[("highway", "residential")])),
            ],
        );
        let graph = build(&objects);
        assert_no_self_loops(&graph);
        assert_route(&graph, NodeId(2), NodeId(1));
        assert_route(&graph, NodeId(1), NodeId(2));
    }

    #[test]
    fn closed_way_with_two_natural_boundaries_connects_branches() {
        let objects = objects(
            &[
                (1, 40.0, -74.0, false),
                (2, 40.001, -73.999, false),
                (3, 40.0, -73.998, true),
                (4, 39.999, -74.0, false),
                (5, 39.999, -73.998, false),
            ],
            vec![
                way(10, &[1, 2, 3, 1], tags(&[("highway", "primary")])),
                way(20, &[1, 4], tags(&[("highway", "residential")])),
                way(30, &[3, 5], tags(&[("highway", "residential")])),
            ],
        );
        let graph = build(&objects);
        assert_no_self_loops(&graph);
        assert_route(&graph, NodeId(2), NodeId(3));
        assert_route(&graph, NodeId(3), NodeId(2));
    }

    #[test]
    fn reports_invalid_way_and_missing_dependencies() {
        let mut objects = objects(
            &[(1, 40.0, -74.0, false), (2, 40.001, -74.0, false)],
            vec![
                way(10, &[1], tags(&[("highway", "primary")])),
                way(20, &[1, 2], tags(&[("highway", "primary")])),
            ],
        );
        let mut report = ImportReport::default();
        build_graph(&objects, &mut report).unwrap();
        assert_eq!(report.invalid_ways, 1);
        assert_eq!(report.retained_ways, 1);

        objects.insert(
            OsmId::Way(WayId(30)),
            OsmObj::Way(way(30, &[1, 99], tags(&[("highway", "primary")]))),
        );
        assert!(matches!(
            build_graph(&objects, &mut ImportReport::default()),
            Err(Error::MissingNodeDependency {
                way_id: 30,
                node_id: 99
            })
        ));
    }

    fn build(objects: &BTreeMap<OsmId, OsmObj>) -> RoadGraph {
        build_graph(objects, &mut ImportReport::default()).unwrap()
    }

    fn assert_no_self_loops(graph: &RoadGraph) {
        assert!(graph.edges().iter().all(|edge| edge.from != edge.to));
    }

    fn assert_route(graph: &RoadGraph, origin: NodeId, destination: NodeId) {
        let request = RouteRequest::new(origin, destination, DrivingPreferences::fastest());
        DijkstraRouter.route_fastest(graph, &request).unwrap();
    }

    fn objects(nodes: &[(i64, f64, f64, bool)], ways: Vec<Way>) -> BTreeMap<OsmId, OsmObj> {
        let mut objects = BTreeMap::new();
        for &(id, lat, lon, signal) in nodes {
            let node = OsmNode {
                id: OsmNodeId(id),
                tags: if signal {
                    tags(&[("highway", "traffic_signals")])
                } else {
                    Tags::new()
                },
                decimicro_lat: (lat * 10_000_000.0).round() as i32,
                decimicro_lon: (lon * 10_000_000.0).round() as i32,
            };
            objects.insert(OsmId::Node(node.id), OsmObj::Node(node));
        }
        for way in ways {
            objects.insert(OsmId::Way(way.id), OsmObj::Way(way));
        }
        objects
    }

    fn way(id: i64, nodes: &[i64], tags: Tags) -> Way {
        Way {
            id: WayId(id),
            tags,
            nodes: nodes.iter().copied().map(OsmNodeId).collect(),
        }
    }

    fn relation(id: i64, restriction: &str, via: OsmId, mut extra_tags: Tags) -> Relation {
        extra_tags.insert("type".into(), "restriction".into());
        extra_tags.insert("restriction".into(), restriction.into());
        Relation {
            id: RelationId(id),
            tags: extra_tags,
            refs: vec![
                Ref {
                    member: OsmId::Way(WayId(10)),
                    role: "from".into(),
                },
                Ref {
                    member: via,
                    role: "via".into(),
                },
                Ref {
                    member: OsmId::Way(WayId(20)),
                    role: "to".into(),
                },
            ],
        }
    }

    fn insert_relation(objects: &mut BTreeMap<OsmId, OsmObj>, relation: Relation) {
        objects.insert(OsmId::Relation(relation.id), OsmObj::Relation(relation));
    }

    fn tags(values: &[(&str, &str)]) -> Tags {
        let mut tags = Tags::new();
        for &(key, value) in values {
            tags.insert(key.into(), value.into());
        }
        tags
    }
}
