use crate::{
    DrivingPreferences, Edge, EdgeId, Error, Maneuver, Result, RoadGraph, edge_heading_change_deg,
    maneuver_context,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TransitionEvaluation {
    pub previous: Option<EdgeId>,
    pub next: EdgeId,
    pub maneuver: Option<Maneuver>,
    pub transition_angle_deg: Option<f64>,
    pub edge_heading_change_deg: f64,
    pub signal_controlled: bool,
    pub traffic_signal: bool,
    pub u_turn_like: bool,
    pub physical_time_s: f64,
    pub highway_penalty_s: f64,
    pub minor_road_penalty_s: f64,
    pub traffic_signal_penalty_s: f64,
    pub turn_penalty_s: f64,
    pub left_turn_penalty_s: f64,
    pub u_turn_penalty_s: f64,
    pub total_cost_s: f64,
}

pub trait CostModel {
    fn transition_cost_s(
        &self,
        graph: &RoadGraph,
        previous: Option<EdgeId>,
        next: EdgeId,
    ) -> Result<f64>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct TravelTimeCost;

impl CostModel for TravelTimeCost {
    fn transition_cost_s(
        &self,
        graph: &RoadGraph,
        _previous: Option<EdgeId>,
        next: EdgeId,
    ) -> Result<f64> {
        edge_travel_time_s(
            graph
                .edge(next)
                .ok_or(Error::InvalidRoute("route references a missing edge"))?,
        )
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PersonalizedCost {
    preferences: DrivingPreferences,
}

impl PersonalizedCost {
    pub fn new(preferences: DrivingPreferences) -> Result<Self> {
        preferences.validate()?;
        Ok(Self { preferences })
    }

    pub fn preferences(self) -> DrivingPreferences {
        self.preferences
    }

    pub fn evaluate_transition(
        &self,
        graph: &RoadGraph,
        previous: Option<EdgeId>,
        next: EdgeId,
    ) -> Result<TransitionEvaluation> {
        let edge = graph
            .edge(next)
            .ok_or(Error::InvalidRoute("route references a missing edge"))?;
        let physical_time_s = edge_travel_time_s(edge)?;
        let highway_penalty_s = if edge.road_class.is_highway() {
            physical_time_s * self.preferences.highway_penalty
        } else {
            0.0
        };
        let minor_road_penalty_s = if edge.road_class.is_minor() {
            physical_time_s * self.preferences.minor_road_penalty
        } else {
            0.0
        };
        let edge_heading_change_deg = edge_heading_change_deg(edge)?;
        let mut maneuver = None;
        let mut transition_angle_deg = None;
        let mut signal_controlled = false;
        let mut traffic_signal = false;
        let mut u_turn_like = edge_heading_change_deg.abs() >= 150.0;
        if let Some(previous_id) = previous {
            if !graph.transition_allowed(previous_id, next)? {
                return Err(Error::ProhibitedTransition {
                    from: previous_id,
                    to: next,
                });
            }
            let previous_edge = graph
                .edge(previous_id)
                .ok_or(Error::InvalidRoute("route references a missing edge"))?;
            let context = maneuver_context(graph, previous_edge, edge)?;
            maneuver = Some(context.maneuver);
            transition_angle_deg = Some(context.transition_angle_deg);
            signal_controlled = context.signal_controlled;
            traffic_signal = previous_edge.traffic_signal_at_end;
            u_turn_like |= context.u_turn_like;
        }

        let traffic_signal_penalty_s = if traffic_signal {
            self.preferences.traffic_signal_penalty_s
        } else {
            0.0
        };
        let mut turn_penalty_s = 0.0;
        let mut left_turn_penalty_s = 0.0;
        let u_turn_penalty_s = if u_turn_like {
            self.preferences.u_turn_penalty_s
        } else {
            if matches!(maneuver, Some(Maneuver::Left | Maneuver::Right)) {
                turn_penalty_s = self.preferences.turn_penalty_s;
            }
            if maneuver == Some(Maneuver::Left) && !signal_controlled {
                left_turn_penalty_s = self.preferences.left_turn_penalty_s;
            }
            0.0
        };
        let total_cost_s = physical_time_s
            + highway_penalty_s
            + minor_road_penalty_s
            + traffic_signal_penalty_s
            + turn_penalty_s
            + left_turn_penalty_s
            + u_turn_penalty_s;
        if !total_cost_s.is_finite() || total_cost_s < 0.0 {
            return Err(Error::InvalidCost(total_cost_s));
        }
        Ok(TransitionEvaluation {
            previous,
            next,
            maneuver,
            transition_angle_deg,
            edge_heading_change_deg,
            signal_controlled,
            traffic_signal,
            u_turn_like,
            physical_time_s,
            highway_penalty_s,
            minor_road_penalty_s,
            traffic_signal_penalty_s,
            turn_penalty_s,
            left_turn_penalty_s,
            u_turn_penalty_s,
            total_cost_s,
        })
    }
}

impl CostModel for PersonalizedCost {
    fn transition_cost_s(
        &self,
        graph: &RoadGraph,
        previous: Option<EdgeId>,
        next: EdgeId,
    ) -> Result<f64> {
        Ok(self
            .evaluate_transition(graph, previous, next)?
            .total_cost_s)
    }
}

pub fn edge_travel_time_s(edge: &Edge) -> Result<f64> {
    let speed_kph = edge
        .speed_limit_kph
        .unwrap_or_else(|| edge.road_class.default_speed_kph());
    if !edge.distance_m.is_finite()
        || edge.distance_m <= 0.0
        || !speed_kph.is_finite()
        || speed_kph <= 0.0
    {
        return Err(Error::InvalidEdgeValue {
            edge: edge.id,
            field: "travel_time_inputs",
            value: edge.distance_m,
        });
    }
    let time_s = edge.distance_m / (speed_kph / 3.6);
    if time_s.is_finite() {
        Ok(time_s)
    } else {
        Err(Error::InvalidCost(time_s))
    }
}

#[cfg(test)]
mod tests {
    use crate::{Coordinate, EdgeId, Node, NodeId, RoadClass};

    use super::*;

    #[test]
    fn explicit_speed_and_fallback_produce_seconds() {
        let mut edge = Edge::new(EdgeId(0), NodeId(0), NodeId(1), 1_000.0, RoadClass::Primary);
        edge.speed_limit_kph = Some(36.0);
        assert!((edge_travel_time_s(&edge).unwrap() - 100.0).abs() < 1e-9);
        edge.speed_limit_kph = None;
        assert!((edge_travel_time_s(&edge).unwrap() - 49.7).abs() < 0.2);
    }

    #[test]
    fn highway_penalty_increases_edge_cost() {
        let graph = two_node_graph(RoadClass::Motorway);
        let preferences = DrivingPreferences {
            highway_penalty: 0.5,
            ..DrivingPreferences::fastest()
        };
        let time = TravelTimeCost
            .transition_cost_s(&graph, None, EdgeId(0))
            .unwrap();
        let personal = PersonalizedCost::new(preferences)
            .unwrap()
            .transition_cost_s(&graph, None, EdgeId(0))
            .unwrap();
        assert!((personal - time * 1.5).abs() < 1e-9);
    }

    #[test]
    fn signalized_left_avoids_extra_left_penalty() {
        let signalized = turn_graph(true, false);
        let unsignalized = turn_graph(false, false);
        let model = PersonalizedCost::new(DrivingPreferences {
            traffic_signal_penalty_s: 10.0,
            turn_penalty_s: 5.0,
            left_turn_penalty_s: 15.0,
            ..DrivingPreferences::fastest()
        })
        .unwrap();
        let signalized = model
            .evaluate_transition(&signalized, Some(EdgeId(0)), EdgeId(1))
            .unwrap();
        let unsignalized = model
            .evaluate_transition(&unsignalized, Some(EdgeId(0)), EdgeId(1))
            .unwrap();
        assert!(signalized.signal_controlled);
        assert_eq!(signalized.traffic_signal_penalty_s, 10.0);
        assert_eq!(signalized.turn_penalty_s, 5.0);
        assert_eq!(signalized.left_turn_penalty_s, 0.0);
        assert_eq!(unsignalized.traffic_signal_penalty_s, 0.0);
        assert_eq!(unsignalized.left_turn_penalty_s, 15.0);
    }

    #[test]
    fn curved_edge_is_charged_as_a_u_turn() {
        let graph = turn_graph(false, true);
        let evaluation = PersonalizedCost::new(DrivingPreferences {
            turn_penalty_s: 5.0,
            u_turn_penalty_s: 120.0,
            ..DrivingPreferences::fastest()
        })
        .unwrap()
        .evaluate_transition(&graph, Some(EdgeId(0)), EdgeId(1))
        .unwrap();
        assert!(evaluation.u_turn_like);
        assert!(evaluation.edge_heading_change_deg.abs() >= 150.0);
        assert_eq!(evaluation.turn_penalty_s, 0.0);
        assert_eq!(evaluation.u_turn_penalty_s, 120.0);
    }

    #[test]
    fn propagated_signal_control_suppresses_left_penalty_without_double_charging_signal() {
        let mut graph = turn_graph(false, false);
        let (nodes, mut edges) = graph.into_parts();
        edges[0].signal_controlled_at_end = true;
        graph = RoadGraph::new(nodes, edges).unwrap();
        let evaluation = PersonalizedCost::new(DrivingPreferences {
            traffic_signal_penalty_s: 10.0,
            turn_penalty_s: 5.0,
            left_turn_penalty_s: 15.0,
            ..DrivingPreferences::fastest()
        })
        .unwrap()
        .evaluate_transition(&graph, Some(EdgeId(0)), EdgeId(1))
        .unwrap();
        assert!(evaluation.signal_controlled);
        assert!(!evaluation.traffic_signal);
        assert_eq!(evaluation.traffic_signal_penalty_s, 0.0);
        assert_eq!(evaluation.left_turn_penalty_s, 0.0);
    }

    fn two_node_graph(road_class: RoadClass) -> RoadGraph {
        let nodes = vec![
            Node::new(NodeId(0), Coordinate::new(0.0, 0.0).unwrap()),
            Node::new(NodeId(1), Coordinate::new(0.0, 0.01).unwrap()),
        ];
        let mut edge = Edge::new(EdgeId(0), NodeId(0), NodeId(1), 1_000.0, road_class);
        edge.speed_limit_kph = Some(36.0);
        RoadGraph::new(nodes, vec![edge]).unwrap()
    }

    fn turn_graph(signal: bool, curved: bool) -> RoadGraph {
        let endpoint = if curved { (0.001, 0.0) } else { (0.0, -0.01) };
        let nodes = vec![
            Node::new(NodeId(0), Coordinate::new(-0.01, 0.0).unwrap()),
            Node::new(NodeId(1), Coordinate::new(0.0, 0.0).unwrap()),
            Node::new(NodeId(2), Coordinate::new(endpoint.0, endpoint.1).unwrap()),
        ];
        let mut previous = Edge::new(EdgeId(0), NodeId(0), NodeId(1), 100.0, RoadClass::Primary);
        previous.speed_limit_kph = Some(36.0);
        previous.traffic_signal_at_end = signal;
        let mut next = Edge::new(EdgeId(1), NodeId(1), NodeId(2), 100.0, RoadClass::Primary);
        next.speed_limit_kph = Some(36.0);
        if curved {
            next.shape = vec![
                Coordinate::new(0.0, 0.0).unwrap(),
                Coordinate::new(0.0, 0.01).unwrap(),
                Coordinate::new(0.001, 0.01).unwrap(),
                Coordinate::new(0.001, 0.0).unwrap(),
            ];
        }
        RoadGraph::new(nodes, vec![previous, next]).unwrap()
    }
}
