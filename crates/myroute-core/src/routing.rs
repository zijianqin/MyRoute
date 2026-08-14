use std::cmp::Ordering;
use std::collections::{BTreeSet, BinaryHeap};

use serde::{Deserialize, Serialize};

use crate::{
    CostModel, DrivingPreferences, EdgeId, Error, NodeId, PersonalizedCost, Result, RoadGraph,
    Route, RouteComparison, TravelTimeCost,
};

pub const PREFERENCE_SCALES: [f64; 5] = [1.0, 0.75, 0.5, 0.25, 0.0];
const BUDGET_EPSILON: f64 = 1e-9;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct RouteRequest {
    pub origin: NodeId,
    pub destination: NodeId,
    pub preferences: DrivingPreferences,
}

impl RouteRequest {
    pub fn new(origin: NodeId, destination: NodeId, preferences: DrivingPreferences) -> Self {
        Self {
            origin,
            destination,
            preferences,
        }
    }
}

pub trait Router {
    fn route(&self, graph: &RoadGraph, request: &RouteRequest) -> Result<Route>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DijkstraRouter;

impl DijkstraRouter {
    pub fn route_fastest(&self, graph: &RoadGraph, request: &RouteRequest) -> Result<Route> {
        self.route_with_cost_model(
            graph,
            request.origin,
            request.destination,
            &TravelTimeCost,
            &request.preferences,
        )
    }

    pub fn route_personalized(&self, graph: &RoadGraph, request: &RouteRequest) -> Result<Route> {
        request.preferences.validate()?;
        let cost = PersonalizedCost::new(request.preferences)?;
        self.route_with_cost_model(
            graph,
            request.origin,
            request.destination,
            &cost,
            &request.preferences,
        )
    }

    pub fn route_comparison(
        &self,
        graph: &RoadGraph,
        request: &RouteRequest,
    ) -> Result<RouteComparison> {
        request.preferences.validate()?;
        let fastest = self.route_fastest(graph, request)?;
        let personalized =
            if let Some(max_extra_time_ratio) = request.preferences.max_extra_time_ratio {
                let budget_s = fastest.travel_time_s * (1.0 + max_extra_time_ratio);
                if !budget_s.is_finite() {
                    return Err(Error::InvalidCost(budget_s));
                }
                self.constrained_personalized(graph, request, budget_s)?
            } else {
                self.route_personalized(graph, request)?
            };
        Ok(RouteComparison::new(fastest, personalized))
    }

    pub fn route_with_cost_model(
        &self,
        graph: &RoadGraph,
        origin: NodeId,
        destination: NodeId,
        cost_model: &dyn CostModel,
        evaluation_preferences: &DrivingPreferences,
    ) -> Result<Route> {
        evaluation_preferences.validate()?;
        if graph.node(origin).is_none() {
            return Err(Error::NodeNotFound(origin));
        }
        if graph.node(destination).is_none() {
            return Err(Error::NodeNotFound(destination));
        }
        if origin == destination {
            return Route::analyze(graph, Vec::new(), evaluation_preferences);
        }

        let start_state = 0;
        let mut distances = vec![f64::INFINITY; graph.edge_count() + 1];
        let mut predecessors = vec![None; graph.edge_count() + 1];
        let mut queue = BinaryHeap::new();
        distances[start_state] = 0.0;
        queue.push(QueueEntry {
            cost_s: 0.0,
            state: start_state,
        });

        while let Some(entry) = queue.pop() {
            if entry.cost_s.total_cmp(&distances[entry.state]) != Ordering::Equal {
                continue;
            }
            let (node, incoming) = if entry.state == start_state {
                (origin, None)
            } else {
                let incoming = EdgeId((entry.state - 1) as u32);
                let node = graph
                    .edge(incoming)
                    .expect("state edge comes from validated graph")
                    .to;
                (node, Some(incoming))
            };
            if node == destination {
                let edges = reconstruct_path(entry.state, &predecessors)?;
                return Route::analyze(graph, edges, evaluation_preferences);
            }

            for &edge_id in graph.outgoing(node)? {
                if let Some(incoming) = incoming
                    && !graph.transition_allowed(incoming, edge_id)?
                {
                    continue;
                }
                let step_cost = cost_model.transition_cost_s(graph, incoming, edge_id)?;
                if !step_cost.is_finite() || step_cost < 0.0 {
                    return Err(Error::InvalidCost(step_cost));
                }
                let next_cost = entry.cost_s + step_cost;
                if !next_cost.is_finite() {
                    return Err(Error::InvalidCost(next_cost));
                }
                let next_state = edge_id.index() + 1;
                if next_cost.total_cmp(&distances[next_state]) == Ordering::Less {
                    distances[next_state] = next_cost;
                    predecessors[next_state] = Some(entry.state);
                    queue.push(QueueEntry {
                        cost_s: next_cost,
                        state: next_state,
                    });
                }
            }
        }

        Err(Error::NoRoute {
            origin,
            destination,
        })
    }

    fn constrained_personalized(
        &self,
        graph: &RoadGraph,
        request: &RouteRequest,
        budget_s: f64,
    ) -> Result<Route> {
        let mut paths = BTreeSet::new();
        let mut candidates = Vec::new();
        for scale in PREFERENCE_SCALES {
            let scaled = request.preferences.scaled(scale)?;
            let model = PersonalizedCost::new(scaled)?;
            let candidate = self.route_with_cost_model(
                graph,
                request.origin,
                request.destination,
                &model,
                &request.preferences,
            )?;
            if paths.insert(candidate.edges.clone())
                && candidate.travel_time_s <= budget_s + budget_s.abs() * BUDGET_EPSILON + 1e-9
            {
                candidates.push(candidate);
            }
        }
        candidates
            .into_iter()
            .min_by(compare_candidates)
            .ok_or(Error::NoRoute {
                origin: request.origin,
                destination: request.destination,
            })
    }
}

impl Router for DijkstraRouter {
    fn route(&self, graph: &RoadGraph, request: &RouteRequest) -> Result<Route> {
        self.route_personalized(graph, request)
    }
}

#[derive(Clone, Copy, Debug)]
struct QueueEntry {
    cost_s: f64,
    state: usize,
}

impl PartialEq for QueueEntry {
    fn eq(&self, other: &Self) -> bool {
        self.cost_s.total_cmp(&other.cost_s) == Ordering::Equal && self.state == other.state
    }
}

impl Eq for QueueEntry {}

impl PartialOrd for QueueEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for QueueEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .cost_s
            .total_cmp(&self.cost_s)
            .then_with(|| other.state.cmp(&self.state))
    }
}

fn reconstruct_path(mut state: usize, predecessors: &[Option<usize>]) -> Result<Vec<EdgeId>> {
    let mut edges = Vec::new();
    while state != 0 {
        edges.push(EdgeId((state - 1) as u32));
        state = predecessors[state].ok_or(Error::InvalidRoute("broken predecessor chain"))?;
    }
    edges.reverse();
    Ok(edges)
}

fn compare_candidates(a: &Route, b: &Route) -> Ordering {
    a.personalized_cost
        .total_cmp(&b.personalized_cost)
        .then_with(|| a.travel_time_s.total_cmp(&b.travel_time_s))
        .then_with(|| a.edges.cmp(&b.edges))
}

#[cfg(test)]
mod tests {
    use crate::{Coordinate, Edge, Node, RoadClass, TurnRestriction};

    use super::*;

    #[test]
    fn fastest_and_highway_averse_choose_different_paths() {
        let graph = two_path_graph();
        let router = DijkstraRouter;
        let fastest = router
            .route_fastest(&graph, &request(DrivingPreferences::fastest()))
            .unwrap();
        assert_eq!(fastest.edges, vec![EdgeId(0), EdgeId(1)]);

        let personalized = router
            .route_personalized(
                &graph,
                &request(DrivingPreferences {
                    highway_penalty: 0.5,
                    ..DrivingPreferences::fastest()
                }),
            )
            .unwrap();
        assert_eq!(personalized.edges, vec![EdgeId(2), EdgeId(3)]);
    }

    #[test]
    fn signal_penalty_can_change_path() {
        let mut graph = two_path_graph();
        let (nodes, mut edges) = graph.into_parts();
        edges[0].road_class = RoadClass::Primary;
        edges[1].road_class = RoadClass::Primary;
        edges[0].traffic_signal_at_end = true;
        graph = RoadGraph::new(nodes, edges).unwrap();
        let route = DijkstraRouter
            .route_personalized(
                &graph,
                &request(DrivingPreferences {
                    traffic_signal_penalty_s: 10.0,
                    ..DrivingPreferences::fastest()
                }),
            )
            .unwrap();
        assert_eq!(route.edges, vec![EdgeId(2), EdgeId(3)]);
    }

    #[test]
    fn left_turn_penalty_uses_incoming_edge_state() {
        let graph = left_turn_graph();
        let fastest = DijkstraRouter
            .route_fastest(&graph, &request_for(0, 4, DrivingPreferences::fastest()))
            .unwrap();
        assert_eq!(fastest.edges, vec![EdgeId(0), EdgeId(1)]);
        let personal = DijkstraRouter
            .route_personalized(
                &graph,
                &request_for(
                    0,
                    4,
                    DrivingPreferences {
                        left_turn_penalty_s: 10.0,
                        ..DrivingPreferences::fastest()
                    },
                ),
            )
            .unwrap();
        assert_eq!(personal.edges, vec![EdgeId(2), EdgeId(3)]);
    }

    #[test]
    fn respects_direction_and_reports_unreachable() {
        let graph = RoadGraph::new(
            nodes(&[(0.0, 0.0), (0.0, 0.01), (1.0, 1.0)]),
            vec![edge(0, 0, 1, 100.0, RoadClass::Primary)],
        )
        .unwrap();
        let error = DijkstraRouter
            .route_fastest(&graph, &request_for(1, 0, DrivingPreferences::fastest()))
            .unwrap_err();
        assert!(matches!(error, Error::NoRoute { .. }));
        let error = DijkstraRouter
            .route_fastest(&graph, &request_for(0, 2, DrivingPreferences::fastest()))
            .unwrap_err();
        assert!(matches!(error, Error::NoRoute { .. }));
    }

    #[test]
    fn parallel_edge_ties_are_deterministic() {
        let graph = RoadGraph::new(
            nodes(&[(0.0, 0.0), (0.0, 0.01)]),
            vec![
                edge(0, 0, 1, 100.0, RoadClass::Primary),
                edge(1, 0, 1, 100.0, RoadClass::Primary),
            ],
        )
        .unwrap();
        let request = request_for(0, 1, DrivingPreferences::fastest());
        for _ in 0..10 {
            assert_eq!(
                DijkstraRouter
                    .route_fastest(&graph, &request)
                    .unwrap()
                    .edges,
                vec![EdgeId(0)]
            );
        }
    }

    #[test]
    fn constrained_selection_falls_back_to_fastest() {
        let graph = two_path_graph();
        let comparison = DijkstraRouter
            .route_comparison(
                &graph,
                &request(DrivingPreferences {
                    highway_penalty: 1.0,
                    max_extra_time_ratio: Some(0.1),
                    ..DrivingPreferences::fastest()
                }),
            )
            .unwrap();
        assert_eq!(comparison.fastest.edges, vec![EdgeId(0), EdgeId(1)]);
        assert_eq!(comparison.personalized.edges, comparison.fastest.edges);
    }

    #[test]
    fn constrained_selection_keeps_feasible_personalized_path() {
        let graph = two_path_graph();
        let comparison = DijkstraRouter
            .route_comparison(
                &graph,
                &request(DrivingPreferences {
                    highway_penalty: 1.0,
                    max_extra_time_ratio: Some(0.25),
                    ..DrivingPreferences::fastest()
                }),
            )
            .unwrap();
        assert_eq!(comparison.personalized.edges, vec![EdgeId(2), EdgeId(3)]);
        assert!(
            comparison.personalized.travel_time_s <= comparison.fastest.travel_time_s * 1.25 + 1e-9
        );
    }

    #[test]
    fn handles_cycles_and_same_node_routes() {
        let graph = RoadGraph::new(
            nodes(&[(0.0, 0.0), (0.0, 0.01), (0.0, 0.02)]),
            vec![
                edge(0, 0, 1, 100.0, RoadClass::Primary),
                edge(1, 1, 0, 100.0, RoadClass::Primary),
                edge(2, 1, 2, 100.0, RoadClass::Primary),
            ],
        )
        .unwrap();
        let route = DijkstraRouter
            .route_fastest(&graph, &request_for(0, 2, DrivingPreferences::fastest()))
            .unwrap();
        assert_eq!(route.edges, vec![EdgeId(0), EdgeId(2)]);
        let empty = DijkstraRouter
            .route_fastest(&graph, &request_for(0, 0, DrivingPreferences::fastest()))
            .unwrap();
        assert!(empty.edges.is_empty());
        assert_eq!(empty.travel_time_s, 0.0);
    }

    #[test]
    fn turn_restrictions_remain_active_for_constrained_search() {
        let graph = RoadGraph::with_turn_restrictions(
            nodes(&[(0.0, 0.0), (0.0, 0.01), (0.0, 0.02), (0.01, 0.01)]),
            vec![
                edge(0, 0, 1, 100.0, RoadClass::Primary),
                edge(1, 1, 2, 100.0, RoadClass::Primary),
                edge(2, 1, 3, 110.0, RoadClass::Primary),
                edge(3, 3, 2, 110.0, RoadClass::Primary),
            ],
            vec![TurnRestriction::new(EdgeId(0), EdgeId(1))],
        )
        .unwrap();
        let comparison = DijkstraRouter
            .route_comparison(
                &graph,
                &request_for(
                    0,
                    2,
                    DrivingPreferences {
                        turn_penalty_s: 5.0,
                        max_extra_time_ratio: Some(0.5),
                        ..DrivingPreferences::fastest()
                    },
                ),
            )
            .unwrap();
        assert_eq!(
            comparison.fastest.edges,
            vec![EdgeId(0), EdgeId(2), EdgeId(3)]
        );
        assert_eq!(comparison.personalized.edges, comparison.fastest.edges);
    }

    #[test]
    fn u_turn_penalty_prevents_signal_avoidance_loop() {
        let mut direct_in = edge(0, 0, 1, 100.0, RoadClass::Primary);
        direct_in.traffic_signal_at_end = true;
        let direct_out = edge(1, 1, 2, 100.0, RoadClass::Primary);
        let detour_in = edge(2, 0, 3, 70.0, RoadClass::Primary);
        let mut u_turn = edge(3, 3, 4, 70.0, RoadClass::Primary);
        u_turn.shape = vec![
            Coordinate::new(-0.005, 0.005).unwrap(),
            Coordinate::new(-0.005, 0.015).unwrap(),
            Coordinate::new(0.005, 0.015).unwrap(),
            Coordinate::new(0.005, 0.005).unwrap(),
        ];
        let detour_out = edge(4, 4, 2, 70.0, RoadClass::Primary);
        let graph = RoadGraph::new(
            nodes(&[
                (-0.01, 0.0),
                (0.0, 0.0),
                (0.01, 0.0),
                (-0.005, 0.005),
                (0.005, 0.005),
            ]),
            vec![direct_in, direct_out, detour_in, u_turn, detour_out],
        )
        .unwrap();
        let fastest = DijkstraRouter
            .route_fastest(&graph, &request_for(0, 2, DrivingPreferences::fastest()))
            .unwrap();
        assert_eq!(fastest.edges, vec![EdgeId(0), EdgeId(1)]);

        let personalized = DijkstraRouter
            .route_personalized(
                &graph,
                &request_for(
                    0,
                    2,
                    DrivingPreferences {
                        traffic_signal_penalty_s: 10.0,
                        u_turn_penalty_s: 120.0,
                        ..DrivingPreferences::fastest()
                    },
                ),
            )
            .unwrap();
        assert_eq!(personalized.edges, fastest.edges);
        assert_eq!(personalized.stats.u_turn_count, 0);
    }

    fn two_path_graph() -> RoadGraph {
        let nodes = nodes(&[(0.0, 0.0), (0.0, 0.01), (0.01, 0.0), (0.01, 0.01)]);
        let mut edges = vec![
            edge(0, 0, 1, 100.0, RoadClass::Motorway),
            edge(1, 1, 3, 100.0, RoadClass::Motorway),
            edge(2, 0, 2, 120.0, RoadClass::Primary),
            edge(3, 2, 3, 120.0, RoadClass::Primary),
        ];
        for edge in &mut edges {
            edge.speed_limit_kph = Some(36.0);
        }
        RoadGraph::new(nodes, edges).unwrap()
    }

    fn left_turn_graph() -> RoadGraph {
        let nodes = nodes(&[
            (0.0, 0.0),
            (0.01, 0.0),
            (0.0, 0.01),
            (0.01, 0.01),
            (0.01, -0.01),
        ]);
        let mut edges = vec![
            edge(0, 0, 1, 100.0, RoadClass::Primary),
            edge(1, 1, 4, 100.0, RoadClass::Primary),
            edge(2, 0, 2, 105.0, RoadClass::Primary),
            edge(3, 2, 4, 105.0, RoadClass::Primary),
        ];
        for edge in &mut edges {
            edge.speed_limit_kph = Some(36.0);
        }
        RoadGraph::new(nodes, edges).unwrap()
    }

    fn request(preferences: DrivingPreferences) -> RouteRequest {
        request_for(0, 3, preferences)
    }

    fn request_for(origin: u32, destination: u32, preferences: DrivingPreferences) -> RouteRequest {
        RouteRequest::new(NodeId(origin), NodeId(destination), preferences)
    }

    fn nodes(values: &[(f64, f64)]) -> Vec<Node> {
        values
            .iter()
            .enumerate()
            .map(|(id, &(lat, lon))| {
                Node::new(NodeId(id as u32), Coordinate::new(lat, lon).unwrap())
            })
            .collect()
    }

    fn edge(id: u32, from: u32, to: u32, distance_m: f64, road_class: RoadClass) -> Edge {
        let mut edge = Edge::new(EdgeId(id), NodeId(from), NodeId(to), distance_m, road_class);
        edge.speed_limit_kph = Some(36.0);
        edge
    }
}
