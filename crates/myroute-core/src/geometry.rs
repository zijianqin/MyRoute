use std::f64::consts::PI;

use serde::{Deserialize, Serialize};

use crate::{Coordinate, Edge, Error, Result, RoadGraph};

const EARTH_RADIUS_M: f64 = 6_371_008.8;
const STRAIGHT_THRESHOLD_DEG: f64 = 30.0;
const U_TURN_THRESHOLD_DEG: f64 = 150.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Maneuver {
    Straight,
    Left,
    Right,
    UTurn,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ManeuverContext {
    pub maneuver: Maneuver,
    pub transition_angle_deg: f64,
    pub edge_heading_change_deg: f64,
    pub signal_controlled: bool,
    pub u_turn_like: bool,
}

pub fn haversine_distance_m(a: Coordinate, b: Coordinate) -> f64 {
    let lat1 = a.lat.to_radians();
    let lat2 = b.lat.to_radians();
    let delta_lat = lat2 - lat1;
    let delta_lon = (b.lon - a.lon).to_radians();
    let half_chord =
        (delta_lat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (delta_lon / 2.0).sin().powi(2);
    let half_chord = half_chord.clamp(0.0, 1.0);
    2.0 * EARTH_RADIUS_M * half_chord.sqrt().asin()
}

pub fn maneuver_between(graph: &RoadGraph, previous: &Edge, next: &Edge) -> Result<Maneuver> {
    Ok(maneuver_context(graph, previous, next)?.maneuver)
}

pub fn maneuver_context(
    graph: &RoadGraph,
    previous: &Edge,
    next: &Edge,
) -> Result<ManeuverContext> {
    let intersection = graph
        .node(previous.to)
        .ok_or(Error::InvalidRoute("maneuver references a missing node"))?
        .coordinate;
    if previous.shape.len() < 2
        || next.shape.len() < 2
        || previous.to != next.from
        || previous.from == previous.to
        || next.from == next.to
    {
        return Err(Error::InvalidRoute(
            "maneuver edges are invalid or disconnected",
        ));
    }
    let before = previous
        .shape
        .iter()
        .rev()
        .copied()
        .find(|&coordinate| !coordinates_match(coordinate, intersection))
        .ok_or(Error::InvalidRoute(
            "incoming maneuver geometry is fully degenerate",
        ))?;
    let after = next
        .shape
        .iter()
        .copied()
        .find(|&coordinate| !coordinates_match(coordinate, intersection))
        .ok_or(Error::InvalidRoute(
            "outgoing maneuver geometry is fully degenerate",
        ))?;
    let transition_angle_deg = maneuver_change_deg(before, intersection, after);
    let maneuver = maneuver_from_change(transition_angle_deg);
    let edge_heading_change_deg = edge_heading_change_deg(next)?;
    Ok(ManeuverContext {
        maneuver,
        transition_angle_deg,
        edge_heading_change_deg,
        signal_controlled: previous.traffic_signal_at_end || previous.signal_controlled_at_end,
        u_turn_like: maneuver == Maneuver::UTurn
            || edge_heading_change_deg.abs() >= U_TURN_THRESHOLD_DEG,
    })
}

pub fn edge_heading_change_deg(edge: &Edge) -> Result<f64> {
    if edge.shape.len() < 2 || edge.from == edge.to {
        return Err(Error::InvalidRoute("edge geometry is invalid"));
    }
    let first = edge
        .shape
        .windows(2)
        .find(|pair| !coordinates_match(pair[0], pair[1]))
        .ok_or(Error::InvalidRoute("edge geometry is fully degenerate"))?;
    let last = edge
        .shape
        .windows(2)
        .rev()
        .find(|pair| !coordinates_match(pair[0], pair[1]))
        .ok_or(Error::InvalidRoute("edge geometry is fully degenerate"))?;
    let reference_lat = (first[0].lat + last[1].lat) / 2.0;
    let (in_x, in_y) = local_vector(first[0], first[1], reference_lat);
    let (out_x, out_y) = local_vector(last[0], last[1], reference_lat);
    Ok(signed_angle_deg(in_x, in_y, out_x, out_y))
}

#[cfg(test)]
fn maneuver_from_points(
    before: Coordinate,
    intersection: Coordinate,
    after: Coordinate,
) -> Maneuver {
    maneuver_from_change(maneuver_change_deg(before, intersection, after))
}

fn maneuver_change_deg(before: Coordinate, intersection: Coordinate, after: Coordinate) -> f64 {
    let (in_x, in_y) = local_vector(before, intersection, intersection.lat);
    let (out_x, out_y) = local_vector(intersection, after, intersection.lat);
    signed_angle_deg(in_x, in_y, out_x, out_y)
}

fn maneuver_from_change(change_deg: f64) -> Maneuver {
    if change_deg.abs() <= STRAIGHT_THRESHOLD_DEG {
        Maneuver::Straight
    } else if change_deg.abs() >= U_TURN_THRESHOLD_DEG {
        Maneuver::UTurn
    } else if change_deg > 0.0 {
        Maneuver::Left
    } else {
        Maneuver::Right
    }
}

fn signed_angle_deg(in_x: f64, in_y: f64, out_x: f64, out_y: f64) -> f64 {
    let cross = in_x * out_y - in_y * out_x;
    let dot = in_x * out_x + in_y * out_y;
    cross.atan2(dot) * 180.0 / PI
}

fn local_vector(from: Coordinate, to: Coordinate, reference_lat: f64) -> (f64, f64) {
    let x = (to.lon - from.lon).to_radians() * reference_lat.to_radians().cos();
    let y = (to.lat - from.lat).to_radians();
    (x, y)
}

pub(crate) fn coordinates_match(a: Coordinate, b: Coordinate) -> bool {
    (a.lat - b.lat).abs() <= 1e-9 && (a.lon - b.lon).abs() <= 1e-9
}

pub(crate) fn unit_sphere_point(coordinate: Coordinate) -> [f64; 3] {
    let lat = coordinate.lat.to_radians();
    let lon = coordinate.lon.to_radians();
    [lat.cos() * lon.cos(), lat.cos() * lon.sin(), lat.sin()]
}

#[cfg(test)]
mod tests {
    use crate::{EdgeId, Node, NodeId, RoadClass};

    use super::*;

    #[test]
    fn haversine_known_distance() {
        let princeton = Coordinate::new(40.3431, -74.6514).unwrap();
        let new_brunswick = Coordinate::new(40.4862, -74.4518).unwrap();
        let distance = haversine_distance_m(princeton, new_brunswick);
        assert!((distance - 23_200.0).abs() < 500.0);
        assert_eq!(haversine_distance_m(princeton, princeton), 0.0);
    }

    #[test]
    fn haversine_antipodal_distances_remain_finite() {
        let antipodal = haversine_distance_m(coordinate(0.0, 0.0), coordinate(0.0, 180.0));
        assert!(antipodal.is_finite());
        assert!((antipodal - PI * EARTH_RADIUS_M).abs() < 1e-6);

        let near_antipodal = haversine_distance_m(
            coordinate(89.999_999, 0.0),
            coordinate(-89.999_999, 179.999_999),
        );
        assert!(near_antipodal.is_finite());
        assert!(near_antipodal <= PI * EARTH_RADIUS_M);
        assert!(near_antipodal > 20_000_000.0);
    }

    #[test]
    fn classifies_cardinal_maneuvers() {
        let center = coordinate(0.0, 0.0);
        assert_eq!(
            maneuver_from_points(coordinate(-1.0, 0.0), center, coordinate(1.0, 0.0)),
            Maneuver::Straight
        );
        assert_eq!(
            maneuver_from_points(coordinate(-1.0, 0.0), center, coordinate(0.0, -1.0)),
            Maneuver::Left
        );
        assert_eq!(
            maneuver_from_points(coordinate(-1.0, 0.0), center, coordinate(0.0, 1.0)),
            Maneuver::Right
        );
        assert_eq!(
            maneuver_from_points(coordinate(-1.0, 0.0), center, coordinate(-1.0, 0.0)),
            Maneuver::UTurn
        );
    }

    #[test]
    fn threshold_boundaries_are_stable() {
        let center = coordinate(0.0, 0.0);
        let point_at = |degrees: f64| {
            let radians = degrees.to_radians();
            coordinate(radians.cos(), -radians.sin())
        };
        assert_eq!(
            maneuver_from_points(coordinate(-1.0, 0.0), center, point_at(30.0)),
            Maneuver::Straight
        );
        assert_eq!(
            maneuver_from_points(coordinate(-1.0, 0.0), center, point_at(31.0)),
            Maneuver::Left
        );
        assert_eq!(
            maneuver_from_points(coordinate(-1.0, 0.0), center, point_at(150.0)),
            Maneuver::UTurn
        );
        assert_eq!(
            maneuver_from_points(coordinate(-1.0, 0.0), center, point_at(151.0)),
            Maneuver::UTurn
        );
    }

    #[test]
    fn detects_heading_reversal_inside_an_edge() {
        let edge = Edge::new(EdgeId(0), NodeId(0), NodeId(1), 100.0, RoadClass::Primary)
            .with_shape(vec![
                coordinate(0.0, 0.0),
                coordinate(0.0, 0.01),
                coordinate(0.001, 0.01),
                coordinate(0.001, 0.0),
            ]);
        assert!(edge_heading_change_deg(&edge).unwrap().abs() >= 150.0);
    }

    #[test]
    fn maneuver_skips_duplicate_intersection_points() {
        let graph = RoadGraph::new(
            vec![node(0, -1.0, 0.0), node(1, 0.0, 0.0), node(2, 0.0, -1.0)],
            vec![
                edge(0, 0, 1).with_shape(vec![
                    coordinate(-1.0, 0.0),
                    coordinate(0.0, 0.0),
                    coordinate(0.0, 0.0),
                ]),
                edge(1, 1, 2).with_shape(vec![
                    coordinate(0.0, 0.0),
                    coordinate(0.0, 0.0),
                    coordinate(0.0, -1.0),
                ]),
            ],
        )
        .unwrap();
        assert_eq!(
            maneuver_between(
                &graph,
                graph.edge(EdgeId(0)).unwrap(),
                graph.edge(EdgeId(1)).unwrap()
            )
            .unwrap(),
            Maneuver::Left
        );
    }

    #[test]
    fn maneuver_rejects_degenerate_self_loop_geometry() {
        let center = coordinate(0.0, 0.0);
        let graph = RoadGraph::new(
            vec![
                Node::new(NodeId(0), center),
                node(1, -1.0, 0.0),
                node(2, 0.0, 1.0),
            ],
            vec![
                edge(0, 0, 0).with_shape(vec![center, center, center]),
                edge(1, 0, 2),
                edge(2, 1, 0),
            ],
        )
        .unwrap();
        assert!(matches!(
            maneuver_between(
                &graph,
                graph.edge(EdgeId(0)).unwrap(),
                graph.edge(EdgeId(1)).unwrap()
            ),
            Err(Error::InvalidRoute(_))
        ));
        assert!(matches!(
            maneuver_between(
                &graph,
                graph.edge(EdgeId(2)).unwrap(),
                graph.edge(EdgeId(0)).unwrap()
            ),
            Err(Error::InvalidRoute(_))
        ));
    }

    fn coordinate(lat: f64, lon: f64) -> Coordinate {
        Coordinate::new(lat, lon).unwrap()
    }

    fn node(id: u32, lat: f64, lon: f64) -> Node {
        Node::new(NodeId(id), coordinate(lat, lon))
    }

    fn edge(id: u32, from: u32, to: u32) -> Edge {
        Edge::new(
            EdgeId(id),
            NodeId(from),
            NodeId(to),
            100.0,
            RoadClass::Primary,
        )
    }
}
