mod cost;
mod geometry;
mod graph;
mod profile;
mod route;
mod routing;

pub use cost::{
    CostModel, PersonalizedCost, TransitionEvaluation, TravelTimeCost, edge_travel_time_s,
};
pub use geometry::{
    Maneuver, ManeuverContext, edge_heading_change_deg, haversine_distance_m, maneuver_between,
    maneuver_context,
};
pub use graph::{
    Coordinate, Edge, EdgeId, Node, NodeId, RoadClass, RoadGraph, SnapResult, TurnRestriction,
};
pub use profile::{DrivingPreferences, DrivingProfile};
pub use route::{Route, RouteComparison, RouteStats};
pub use routing::{DijkstraRouter, RouteRequest, Router};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(
        "invalid coordinate ({lat}, {lon}): latitude must be -90..=90 and longitude -180..=180"
    )]
    InvalidCoordinate { lat: f64, lon: f64 },
    #[error("node IDs must be dense: expected {expected}, found {actual}")]
    NonDenseNodeId { expected: u32, actual: u32 },
    #[error("edge IDs must be dense: expected {expected}, found {actual}")]
    NonDenseEdgeId { expected: u32, actual: u32 },
    #[error("graph has too many {kind} for u32 IDs: {count}")]
    IdCapacity { kind: &'static str, count: usize },
    #[error("edge {edge:?} references missing node {node:?}")]
    MissingNode { edge: EdgeId, node: NodeId },
    #[error("edge {edge:?} has invalid {field}: {value}")]
    InvalidEdgeValue {
        edge: EdgeId,
        field: &'static str,
        value: f64,
    },
    #[error("edge {edge:?} geometry must contain at least two coordinates")]
    InvalidEdgeGeometry { edge: EdgeId },
    #[error("edge {edge:?} geometry does not start and end at its graph nodes")]
    EdgeGeometryEndpoints { edge: EdgeId },
    #[error("turn restriction references missing edge {0:?}")]
    MissingRestrictionEdge(EdgeId),
    #[error("turn restriction edges {from:?} and {to:?} are not contiguous")]
    InvalidTurnRestriction { from: EdgeId, to: EdgeId },
    #[error("graph has no routable edges")]
    EmptyGraph,
    #[error("node {0:?} is not in the graph")]
    NodeNotFound(NodeId),
    #[error(
        "invalid driving preference `{field}`: expected a finite, nonnegative value, found {value}"
    )]
    InvalidPreference { field: &'static str, value: f64 },
    #[error("invalid route cost {0}")]
    InvalidCost(f64),
    #[error("no route from {origin:?} to {destination:?}")]
    NoRoute { origin: NodeId, destination: NodeId },
    #[error("cannot snap to an empty road graph")]
    NoRoutableNode,
    #[error(
        "nearest road node is {distance_m:.1} m away, exceeding the {max_distance_m:.1} m limit"
    )]
    SnapTooFar {
        distance_m: f64,
        max_distance_m: f64,
    },
    #[error("invalid snap distance limit {0}")]
    InvalidSnapDistance(f64),
    #[error("invalid route: {0}")]
    InvalidRoute(&'static str),
    #[error("route transition from edge {from:?} to {to:?} is prohibited")]
    ProhibitedTransition { from: EdgeId, to: EdgeId },
    #[error("invalid TOML profile: {0}")]
    InvalidProfileToml(#[from] toml::de::Error),
}
