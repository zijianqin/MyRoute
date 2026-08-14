use rstar::{AABB, PointDistance, RTree, RTreeObject};
use serde::{Deserialize, Deserializer, Serialize};

use crate::geometry::{coordinates_match, unit_sphere_point};
use crate::{Error, Result, haversine_distance_m};

pub const DEFAULT_MAX_SNAP_DISTANCE_M: f64 = 1_000.0;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NodeId(pub u32);

impl NodeId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

impl From<u32> for NodeId {
    fn from(value: u32) -> Self {
        Self(value)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EdgeId(pub u32);

impl EdgeId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

impl From<u32> for EdgeId {
    fn from(value: u32) -> Self {
        Self(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Coordinate {
    pub lat: f64,
    pub lon: f64,
}

impl Coordinate {
    pub fn new(lat: f64, lon: f64) -> Result<Self> {
        let coordinate = Self { lat, lon };
        coordinate.validate()?;
        Ok(coordinate)
    }

    pub fn validate(self) -> Result<()> {
        if self.lat.is_finite()
            && self.lon.is_finite()
            && (-90.0..=90.0).contains(&self.lat)
            && (-180.0..=180.0).contains(&self.lon)
        {
            Ok(())
        } else {
            Err(Error::InvalidCoordinate {
                lat: self.lat,
                lon: self.lon,
            })
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub coordinate: Coordinate,
}

impl Node {
    pub fn new(id: NodeId, coordinate: Coordinate) -> Self {
        Self { id, coordinate }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub enum RoadClass {
    Motorway,
    Trunk,
    Primary,
    Secondary,
    Tertiary,
    Residential,
    Service,
    Other,
}

impl RoadClass {
    pub fn is_highway(self) -> bool {
        matches!(self, Self::Motorway | Self::Trunk)
    }

    pub fn is_minor(self) -> bool {
        matches!(self, Self::Residential | Self::Service | Self::Other)
    }

    pub fn default_speed_kph(self) -> f64 {
        match self {
            Self::Motorway => 104.607_36,
            Self::Trunk => 88.513_92,
            Self::Primary => 72.420_48,
            Self::Secondary => 56.327_04,
            Self::Tertiary => 48.280_32,
            Self::Residential | Self::Other => 40.233_6,
            Self::Service => 24.140_16,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    pub id: EdgeId,
    pub from: NodeId,
    pub to: NodeId,
    pub distance_m: f64,
    pub road_class: RoadClass,
    pub speed_limit_kph: Option<f64>,
    pub lanes: Option<u8>,
    pub name: Option<String>,
    pub source_way_id: Option<i64>,
    pub traffic_signal_at_end: bool,
    pub signal_controlled_at_end: bool,
    pub shape: Vec<Coordinate>,
}

impl Edge {
    pub fn new(
        id: EdgeId,
        from: NodeId,
        to: NodeId,
        distance_m: f64,
        road_class: RoadClass,
    ) -> Self {
        Self {
            id,
            from,
            to,
            distance_m,
            road_class,
            speed_limit_kph: None,
            lanes: None,
            name: None,
            source_way_id: None,
            traffic_signal_at_end: false,
            signal_controlled_at_end: false,
            shape: Vec::new(),
        }
    }

    pub fn with_shape(mut self, shape: Vec<Coordinate>) -> Self {
        self.shape = shape;
        self
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct TurnRestriction {
    pub from: EdgeId,
    pub to: EdgeId,
}

impl TurnRestriction {
    pub const fn new(from: EdgeId, to: EdgeId) -> Self {
        Self { from, to }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SnapResult {
    pub node: NodeId,
    pub coordinate: Coordinate,
    pub distance_m: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct RoadGraph {
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    turn_restrictions: Vec<TurnRestriction>,
    #[serde(skip)]
    outgoing: Vec<Vec<EdgeId>>,
    #[serde(skip)]
    forbidden_outgoing: Vec<Vec<EdgeId>>,
    #[serde(skip)]
    spatial_index: RTree<SpatialNode>,
}

impl RoadGraph {
    pub fn new(nodes: Vec<Node>, edges: Vec<Edge>) -> Result<Self> {
        Self::with_turn_restrictions(nodes, edges, Vec::new())
    }

    pub fn with_turn_restrictions(
        nodes: Vec<Node>,
        mut edges: Vec<Edge>,
        mut turn_restrictions: Vec<TurnRestriction>,
    ) -> Result<Self> {
        validate_capacity("nodes", nodes.len())?;
        validate_capacity("edges", edges.len())?;

        for (index, node) in nodes.iter().enumerate() {
            let expected = u32::try_from(index).map_err(|_| Error::IdCapacity {
                kind: "nodes",
                count: nodes.len(),
            })?;
            if node.id.0 != expected {
                return Err(Error::NonDenseNodeId {
                    expected,
                    actual: node.id.0,
                });
            }
            node.coordinate.validate()?;
        }
        if edges.is_empty() {
            return Err(Error::EmptyGraph);
        }

        let edge_count = edges.len();
        let mut outgoing = vec![Vec::new(); nodes.len()];
        let mut routable = vec![false; nodes.len()];
        for (index, edge) in edges.iter_mut().enumerate() {
            let expected = u32::try_from(index).map_err(|_| Error::IdCapacity {
                kind: "edges",
                count: edge_count,
            })?;
            if edge.id.0 != expected {
                return Err(Error::NonDenseEdgeId {
                    expected,
                    actual: edge.id.0,
                });
            }
            validate_edge(edge, &nodes)?;
            if edge.shape.is_empty() {
                edge.shape = vec![
                    nodes[edge.from.index()].coordinate,
                    nodes[edge.to.index()].coordinate,
                ];
            }
            validate_shape(edge, &nodes)?;
            outgoing[edge.from.index()].push(edge.id);
            routable[edge.from.index()] = true;
            routable[edge.to.index()] = true;
        }

        let spatial_index = RTree::bulk_load(
            nodes
                .iter()
                .zip(routable)
                .filter(|(_, routable)| *routable)
                .map(|(node, _)| SpatialNode {
                    point: unit_sphere_point(node.coordinate),
                    node: node.id,
                })
                .collect(),
        );

        turn_restrictions.sort_unstable();
        turn_restrictions.dedup();
        let mut forbidden_outgoing = vec![Vec::new(); edges.len()];
        for restriction in &turn_restrictions {
            let previous = edges
                .get(restriction.from.index())
                .ok_or(Error::MissingRestrictionEdge(restriction.from))?;
            let next = edges
                .get(restriction.to.index())
                .ok_or(Error::MissingRestrictionEdge(restriction.to))?;
            if previous.to != next.from {
                return Err(Error::InvalidTurnRestriction {
                    from: restriction.from,
                    to: restriction.to,
                });
            }
            forbidden_outgoing[restriction.from.index()].push(restriction.to);
        }

        Ok(Self {
            nodes,
            edges,
            turn_restrictions,
            outgoing,
            forbidden_outgoing,
            spatial_index,
        })
    }

    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    pub fn edges(&self) -> &[Edge] {
        &self.edges
    }

    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(id.index())
    }

    pub fn edge(&self, id: EdgeId) -> Option<&Edge> {
        self.edges.get(id.index())
    }

    pub fn turn_restrictions(&self) -> &[TurnRestriction] {
        &self.turn_restrictions
    }

    pub fn transition_allowed(&self, previous: EdgeId, next: EdgeId) -> Result<bool> {
        let previous_edge = self
            .edge(previous)
            .ok_or(Error::InvalidRoute("route references a missing edge"))?;
        let next_edge = self
            .edge(next)
            .ok_or(Error::InvalidRoute("route references a missing edge"))?;
        if previous_edge.to != next_edge.from {
            return Err(Error::InvalidRoute("route edges are not contiguous"));
        }
        Ok(previous_edge.from != next_edge.to
            && self.forbidden_outgoing[previous.index()]
                .binary_search(&next)
                .is_err())
    }

    pub fn outgoing(&self, id: NodeId) -> Result<&[EdgeId]> {
        self.outgoing
            .get(id.index())
            .map(Vec::as_slice)
            .ok_or(Error::NodeNotFound(id))
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    pub fn nearest_road_node(&self, location: Coordinate) -> Result<SnapResult> {
        self.nearest_road_node_with_limit(location, DEFAULT_MAX_SNAP_DISTANCE_M)
    }

    pub fn nearest_road_node_with_limit(
        &self,
        location: Coordinate,
        max_distance_m: f64,
    ) -> Result<SnapResult> {
        location.validate()?;
        if !max_distance_m.is_finite() || max_distance_m < 0.0 {
            return Err(Error::InvalidSnapDistance(max_distance_m));
        }
        let nearest = self
            .spatial_index
            .nearest_neighbor(&unit_sphere_point(location))
            .ok_or(Error::NoRoutableNode)?;
        let coordinate = self.nodes[nearest.node.index()].coordinate;
        let distance_m = haversine_distance_m(location, coordinate);
        if distance_m > max_distance_m {
            return Err(Error::SnapTooFar {
                distance_m,
                max_distance_m,
            });
        }
        Ok(SnapResult {
            node: nearest.node,
            coordinate,
            distance_m,
        })
    }

    pub fn into_parts(self) -> (Vec<Node>, Vec<Edge>) {
        (self.nodes, self.edges)
    }

    pub fn into_parts_with_turn_restrictions(self) -> (Vec<Node>, Vec<Edge>, Vec<TurnRestriction>) {
        (self.nodes, self.edges, self.turn_restrictions)
    }
}

#[derive(Deserialize)]
struct SerializedGraph {
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    turn_restrictions: Vec<TurnRestriction>,
}

impl<'de> Deserialize<'de> for RoadGraph {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let data = SerializedGraph::deserialize(deserializer)?;
        Self::with_turn_restrictions(data.nodes, data.edges, data.turn_restrictions)
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug)]
struct SpatialNode {
    point: [f64; 3],
    node: NodeId,
}

impl RTreeObject for SpatialNode {
    type Envelope = AABB<[f64; 3]>;

    fn envelope(&self) -> Self::Envelope {
        AABB::from_point(self.point)
    }
}

impl PointDistance for SpatialNode {
    fn distance_2(&self, point: &[f64; 3]) -> f64 {
        self.point.distance_2(point)
    }
}

fn validate_capacity(kind: &'static str, count: usize) -> Result<()> {
    if count > u32::MAX as usize + 1 {
        Err(Error::IdCapacity { kind, count })
    } else {
        Ok(())
    }
}

fn validate_edge(edge: &Edge, nodes: &[Node]) -> Result<()> {
    for node in [edge.from, edge.to] {
        if node.index() >= nodes.len() {
            return Err(Error::MissingNode {
                edge: edge.id,
                node,
            });
        }
    }
    if !edge.distance_m.is_finite() || edge.distance_m <= 0.0 {
        return Err(Error::InvalidEdgeValue {
            edge: edge.id,
            field: "distance_m",
            value: edge.distance_m,
        });
    }
    if let Some(speed) = edge.speed_limit_kph
        && (!speed.is_finite() || speed <= 0.0)
    {
        return Err(Error::InvalidEdgeValue {
            edge: edge.id,
            field: "speed_limit_kph",
            value: speed,
        });
    }
    if edge.lanes == Some(0) {
        return Err(Error::InvalidEdgeValue {
            edge: edge.id,
            field: "lanes",
            value: 0.0,
        });
    }
    Ok(())
}

fn validate_shape(edge: &Edge, nodes: &[Node]) -> Result<()> {
    if edge.shape.len() < 2 {
        return Err(Error::InvalidEdgeGeometry { edge: edge.id });
    }
    for coordinate in &edge.shape {
        coordinate.validate()?;
    }
    if !coordinates_match(edge.shape[0], nodes[edge.from.index()].coordinate)
        || !coordinates_match(
            *edge.shape.last().expect("nonempty shape"),
            nodes[edge.to.index()].coordinate,
        )
    {
        return Err(Error::EdgeGeometryEndpoints { edge: edge.id });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_dense_and_invalid_graph_data() {
        let invalid_nodes = vec![Node::new(NodeId(1), coordinate(0.0, 0.0))];
        assert!(matches!(
            RoadGraph::new(invalid_nodes, vec![edge(0, 0, 0, 1.0)]),
            Err(Error::NonDenseNodeId { .. })
        ));

        let nodes = nodes(&[(0.0, 0.0), (0.0, 0.1)]);
        assert!(matches!(
            RoadGraph::new(nodes.clone(), vec![edge(0, 0, 2, 1.0)]),
            Err(Error::MissingNode { .. })
        ));
        assert!(matches!(
            RoadGraph::new(nodes, vec![edge(0, 0, 1, f64::NAN)]),
            Err(Error::InvalidEdgeValue { .. })
        ));
    }

    #[test]
    fn fills_direct_edge_geometry_and_adjacency() {
        let graph =
            RoadGraph::new(nodes(&[(0.0, 0.0), (0.0, 0.1)]), vec![edge(0, 0, 1, 100.0)]).unwrap();
        assert_eq!(graph.edge(EdgeId(0)).unwrap().shape.len(), 2);
        assert_eq!(graph.outgoing(NodeId(0)).unwrap(), &[EdgeId(0)]);
        assert!(graph.outgoing(NodeId(1)).unwrap().is_empty());
    }

    #[test]
    fn validates_and_indexes_turn_restrictions() {
        let graph = RoadGraph::with_turn_restrictions(
            nodes(&[(0.0, 0.0), (0.0, 0.1), (0.1, 0.1)]),
            vec![edge(0, 0, 1, 100.0), edge(1, 1, 2, 100.0)],
            vec![TurnRestriction::new(EdgeId(0), EdgeId(1))],
        )
        .unwrap();
        assert!(!graph.transition_allowed(EdgeId(0), EdgeId(1)).unwrap());
        assert_eq!(graph.turn_restrictions().len(), 1);

        let error = RoadGraph::with_turn_restrictions(
            nodes(&[(0.0, 0.0), (0.0, 0.1), (0.1, 0.1)]),
            vec![edge(0, 0, 1, 100.0), edge(1, 0, 2, 100.0)],
            vec![TurnRestriction::new(EdgeId(0), EdgeId(1))],
        )
        .unwrap_err();
        assert!(matches!(error, Error::InvalidTurnRestriction { .. }));
    }

    #[test]
    fn rejects_immediate_reverse_transition() {
        let graph = RoadGraph::new(
            nodes(&[(0.0, 0.0), (0.0, 0.1)]),
            vec![edge(0, 0, 1, 100.0), edge(1, 1, 0, 100.0)],
        )
        .unwrap();
        assert!(!graph.transition_allowed(EdgeId(0), EdgeId(1)).unwrap());
    }

    #[test]
    fn snaps_only_within_limit() {
        let graph = RoadGraph::new(
            nodes(&[(40.0, -74.0), (40.0, -73.99)]),
            vec![edge(0, 0, 1, 850.0)],
        )
        .unwrap();
        let snapped = graph
            .nearest_road_node(coordinate(40.0001, -74.0001))
            .unwrap();
        assert_eq!(snapped.node, NodeId(0));
        assert!(snapped.distance_m < 20.0);
        assert!(matches!(
            graph.nearest_road_node_with_limit(coordinate(41.0, -74.0), 1_000.0),
            Err(Error::SnapTooFar { .. })
        ));
    }

    fn coordinate(lat: f64, lon: f64) -> Coordinate {
        Coordinate::new(lat, lon).unwrap()
    }

    fn nodes(values: &[(f64, f64)]) -> Vec<Node> {
        values
            .iter()
            .enumerate()
            .map(|(id, &(lat, lon))| Node::new(NodeId(id as u32), coordinate(lat, lon)))
            .collect()
    }

    fn edge(id: u32, from: u32, to: u32, distance_m: f64) -> Edge {
        Edge::new(
            EdgeId(id),
            NodeId(from),
            NodeId(to),
            distance_m,
            RoadClass::Residential,
        )
    }
}
