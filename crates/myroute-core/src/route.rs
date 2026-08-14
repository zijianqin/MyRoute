use serde::{Deserialize, Serialize};

use crate::{
    DrivingPreferences, EdgeId, Error, Maneuver, PersonalizedCost, Result, RoadGraph,
    edge_travel_time_s,
};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RouteStats {
    pub highway_distance_m: f64,
    pub minor_road_distance_m: f64,
    pub traffic_signal_count: usize,
    pub left_turn_count: usize,
    pub right_turn_count: usize,
    pub u_turn_count: usize,
    pub signalized_left_turn_count: usize,
    pub unsignalized_left_turn_count: usize,
    pub general_turn_penalty_s: f64,
    pub left_turn_penalty_s: f64,
    pub u_turn_penalty_s: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Route {
    pub edges: Vec<EdgeId>,
    pub distance_m: f64,
    pub travel_time_s: f64,
    pub personalized_cost: f64,
    pub stats: RouteStats,
}

impl Route {
    pub fn analyze(
        graph: &RoadGraph,
        edges: Vec<EdgeId>,
        preferences: &DrivingPreferences,
    ) -> Result<Self> {
        preferences.validate()?;
        let cost_model = PersonalizedCost::new(*preferences)?;
        let mut route = Self {
            edges,
            distance_m: 0.0,
            travel_time_s: 0.0,
            personalized_cost: 0.0,
            stats: RouteStats::default(),
        };

        let mut previous = None;
        for &edge_id in &route.edges {
            let edge = graph
                .edge(edge_id)
                .ok_or(Error::InvalidRoute("route references a missing edge"))?;
            if let Some(previous_id) = previous {
                let previous_edge = graph
                    .edge(previous_id)
                    .ok_or(Error::InvalidRoute("route references a missing edge"))?;
                if previous_edge.to != edge.from {
                    return Err(Error::InvalidRoute("route edges are not contiguous"));
                }
            }
            let evaluation = cost_model.evaluate_transition(graph, previous, edge_id)?;
            route.distance_m += edge.distance_m;
            route.travel_time_s += edge_travel_time_s(edge)?;
            route.personalized_cost += evaluation.total_cost_s;
            if edge.road_class.is_highway() {
                route.stats.highway_distance_m += edge.distance_m;
            }
            if edge.road_class.is_minor() {
                route.stats.minor_road_distance_m += edge.distance_m;
            }
            if evaluation.traffic_signal {
                route.stats.traffic_signal_count += 1;
            }
            if evaluation.u_turn_like {
                route.stats.u_turn_count += 1;
            } else {
                match evaluation.maneuver {
                    Some(Maneuver::Left) => {
                        route.stats.left_turn_count += 1;
                        if evaluation.signal_controlled {
                            route.stats.signalized_left_turn_count += 1;
                        } else {
                            route.stats.unsignalized_left_turn_count += 1;
                        }
                    }
                    Some(Maneuver::Right) => route.stats.right_turn_count += 1,
                    Some(Maneuver::Straight | Maneuver::UTurn) | None => {}
                }
            }
            route.stats.general_turn_penalty_s += evaluation.turn_penalty_s;
            route.stats.left_turn_penalty_s += evaluation.left_turn_penalty_s;
            route.stats.u_turn_penalty_s += evaluation.u_turn_penalty_s;
            previous = Some(edge_id);
        }
        for value in [
            route.distance_m,
            route.travel_time_s,
            route.personalized_cost,
            route.stats.highway_distance_m,
            route.stats.minor_road_distance_m,
            route.stats.general_turn_penalty_s,
            route.stats.left_turn_penalty_s,
            route.stats.u_turn_penalty_s,
        ] {
            if !value.is_finite() {
                return Err(Error::InvalidCost(value));
            }
        }
        Ok(route)
    }

    pub fn geometry(&self, graph: &RoadGraph) -> Result<Vec<crate::Coordinate>> {
        let mut geometry = Vec::new();
        let mut previous_to = None;
        for &edge_id in &self.edges {
            let edge = graph
                .edge(edge_id)
                .ok_or(Error::InvalidRoute("route references a missing edge"))?;
            if previous_to.is_some_and(|node| node != edge.from) {
                return Err(Error::InvalidRoute("route edges are not contiguous"));
            }
            if geometry.is_empty() {
                geometry.extend_from_slice(&edge.shape);
            } else {
                geometry.extend_from_slice(&edge.shape[1..]);
            }
            previous_to = Some(edge.to);
        }
        Ok(geometry)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RouteComparison {
    pub fastest: Route,
    pub personalized: Route,
    pub fastest_preference_score: f64,
    pub personalized_preference_score: f64,
}

impl RouteComparison {
    pub fn new(fastest: Route, personalized: Route) -> Self {
        let personalized_preference_score = if fastest.personalized_cost == 0.0 {
            1.0
        } else {
            personalized.personalized_cost / fastest.personalized_cost
        };
        Self {
            fastest,
            personalized,
            fastest_preference_score: 1.0,
            personalized_preference_score,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{Coordinate, Edge, Node, NodeId, RoadClass};

    use super::*;

    #[test]
    fn analyzes_statistics_and_geometry() {
        let graph = graph();
        let route = Route::analyze(
            &graph,
            vec![EdgeId(0), EdgeId(1)],
            &DrivingPreferences::fastest(),
        )
        .unwrap();
        assert_eq!(route.distance_m, 200.0);
        assert_eq!(route.stats.highway_distance_m, 100.0);
        assert_eq!(route.stats.minor_road_distance_m, 100.0);
        assert_eq!(route.stats.traffic_signal_count, 1);
        assert_eq!(route.stats.left_turn_count, 1);
        assert_eq!(route.stats.signalized_left_turn_count, 1);
        assert_eq!(route.stats.unsignalized_left_turn_count, 0);
        assert_eq!(route.stats.right_turn_count, 0);
        assert_eq!(route.stats.u_turn_count, 0);
        assert_eq!(route.geometry(&graph).unwrap().len(), 3);
    }

    #[test]
    fn rejects_noncontiguous_edges() {
        let graph = graph();
        let error = Route::analyze(
            &graph,
            vec![EdgeId(1), EdgeId(0)],
            &DrivingPreferences::fastest(),
        )
        .unwrap_err();
        assert!(matches!(error, Error::InvalidRoute(_)));
    }

    fn graph() -> RoadGraph {
        let nodes = vec![node(0, 0.0, 0.0), node(1, 0.01, 0.0), node(2, 0.01, -0.01)];
        let mut first = Edge::new(EdgeId(0), NodeId(0), NodeId(1), 100.0, RoadClass::Motorway);
        first.speed_limit_kph = Some(36.0);
        first.traffic_signal_at_end = true;
        let mut second = Edge::new(
            EdgeId(1),
            NodeId(1),
            NodeId(2),
            100.0,
            RoadClass::Residential,
        );
        second.speed_limit_kph = Some(36.0);
        RoadGraph::new(nodes, vec![first, second]).unwrap()
    }

    fn node(id: u32, lat: f64, lon: f64) -> Node {
        Node::new(NodeId(id), Coordinate::new(lat, lon).unwrap())
    }
}
