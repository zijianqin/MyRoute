# MyRoute v0.1 — Design and Implementation Specification

## 1. Overview

**MyRoute** is an open-source personalized navigation system that computes driving routes according to a user's preferences rather than optimizing exclusively for minimum travel time.

Version 0.1 is intentionally small. It is **not** a turn-by-turn navigation application and does not run on an iPhone or CarPlay. Instead, it is a desktop development prototype that demonstrates the project's central idea:

> Given an origin, destination, and driving preferences, compute and visualize a route that trades additional travel time for characteristics the user prefers.

For example, a user may prefer to:

- avoid highways;
- avoid minor or narrow roads;
- avoid traffic lights;
- avoid left turns;
- prefer larger, easier-to-drive roads;
- tolerate up to 10 additional minutes compared with the fastest route.

The primary goal of v0.1 is to establish a clean routing architecture that can later support a web UI, learned preferences, real-time navigation, iOS, and CarPlay.

---

## 2. Goals

MyRoute v0.1 should support the following workflow:

```bash
myroute route \
    --from "Princeton, NJ" \
    --to "New Brunswick, NJ" \
    --profile chill
```

or:

```bash
myroute route \
    --from "40.3431,-74.6514" \
    --to "40.4862,-74.4518" \
    --highway-penalty 0.6 \
    --minor-road-penalty 0.8 \
    --traffic-light-penalty 15 \
    --left-turn-penalty 20 \
    --max-extra-time 0.15
```

The program should produce:

```text
MyRoute
Princeton, NJ → New Brunswick, NJ

Fastest route:
    31.4 min
    24.8 mi

Personalized route:
    34.7 min
    25.9 mi

Difference:
    +3.3 min
    +1.1 mi

Route characteristics:

                     Fastest      MyRoute
Highway distance      18.1 mi      11.3 mi
Traffic lights         14            8
Left turns              7            3
Minor roads           3.2 mi       1.1 mi

Preference score:
    Fastest: 1.00
    MyRoute: 0.72
```

The route should also be written to an output file that can be visualized on a map.

---

## 3. Non-Goals

The following features are explicitly outside the scope of v0.1:

- iPhone application;
- CarPlay;
- live GPS tracking;
- turn-by-turn navigation;
- voice instructions;
- real-time traffic;
- automatic rerouting;
- map matching;
- user accounts;
- cloud services;
- machine-learned preferences;
- route-history learning;
- mobile UI;
- global map coverage;
- production-quality geocoding.

These should not influence the architecture enough to make v0.1 unnecessarily complicated.

---

## 4. Basic Design Principle

Traditional routing approximately minimizes:

\[
C(R) = T(R)
\]

where \(T(R)\) is estimated route travel time.

MyRoute instead minimizes a personalized cost:

\[
C(R) =
T(R)
+
P_{\text{road}}(R)
+
P_{\text{intersection}}(R)
+
P_{\text{maneuver}}(R)
+
P_{\text{user}}(R)
\]

For v0.1:

\[
C(R)
=
T(R)
+
w_h H(R)
+
w_m M(R)
+
w_s S(R)
+
w_l L(R)
\]

where:

- \(T(R)\): estimated travel time;
- \(H(R)\): highway exposure;
- \(M(R)\): minor-road exposure;
- \(S(R)\): number of traffic lights;
- \(L(R)\): number of left turns;
- \(w_h,w_m,w_s,w_l\): user-configurable penalties.

The design should allow additional features to be introduced later without rewriting the routing engine.

---

## 5. Recommended Technology Stack

### 5.1 Core implementation

Use **Rust**.

Recommended toolchain:

```text
Rust stable
Cargo
Tokio only where asynchronous operations are useful
Serde for serialization
Clap for CLI parsing
Tracing for logging
```

Rust is appropriate because the routing core can eventually become a reusable library for:

- desktop applications;
- servers;
- iOS through FFI;
- Android;
- WebAssembly.

The v0.1 codebase should therefore separate library functionality from the CLI.

### 5.2 Map data

Use **OpenStreetMap** as the source of road-network data.

For v0.1, restrict the supported dataset to a small geographic region, such as:

```text
New Jersey
```

or even:

```text
Princeton + surrounding ~100 km
```

A regional `.osm.pbf` file is sufficient.

Do not attempt to process the entire planet.

---

## 6. High-Level Architecture

```text
                         ┌─────────────────────┐
                         │       CLI           │
                         │ myroute route ...   │
                         └──────────┬──────────┘
                                    │
                                    ▼
                         ┌─────────────────────┐
                         │   Route Request     │
                         │                     │
                         │ origin              │
                         │ destination         │
                         │ preferences         │
                         └──────────┬──────────┘
                                    │
                    ┌───────────────┴───────────────┐
                    │                               │
                    ▼                               ▼
           ┌────────────────┐              ┌────────────────┐
           │  Fast Router   │              │ Personal Router│
           └────────┬───────┘              └────────┬───────┘
                    │                               │
                    └───────────────┬───────────────┘
                                    ▼
                         ┌─────────────────────┐
                         │     Road Graph      │
                         │                     │
                         │ nodes               │
                         │ edges               │
                         │ OSM attributes      │
                         └──────────┬──────────┘
                                    │
                                    ▼
                         ┌─────────────────────┐
                         │  Route Analyzer     │
                         │                     │
                         │ time                │
                         │ distance            │
                         │ highways            │
                         │ lights              │
                         │ turns               │
                         └──────────┬──────────┘
                                    │
                                    ▼
                         ┌─────────────────────┐
                         │ Output / Visualizer │
                         └─────────────────────┘
```

---

## 7. Repository Structure

The repository should initially look like:

```text
myroute/
├── Cargo.toml
├── README.md
├── LICENSE
│
├── crates/
│   ├── myroute-core/
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── graph.rs
│   │       ├── routing.rs
│   │       ├── cost.rs
│   │       ├── profile.rs
│   │       ├── route.rs
│   │       └── geometry.rs
│   │
│   ├── myroute-osm/
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── parser.rs
│   │       └── builder.rs
│   │
│   └── myroute-cli/
│       └── src/
│           └── main.rs
│
├── profiles/
│   ├── fastest.toml
│   ├── chill.toml
│   └── no-highway.toml
│
├── data/
│   └── README.md
│
├── examples/
│
└── tests/
```

The most important architectural rule is:

```text
myroute-core must not depend on the CLI.
```

The routing library should eventually be usable independently.

---

## 8. Road Graph

The road network should be represented as a directed graph.

### 8.1 Node

```rust
pub struct Node {
    pub id: NodeId,
    pub lat: f64,
    pub lon: f64,
}
```

A node generally corresponds to an OSM road intersection or geometry point.

### 8.2 Edge

```rust
pub struct Edge {
    pub id: EdgeId,

    pub from: NodeId,
    pub to: NodeId,

    pub distance_m: f64,

    pub road_class: RoadClass,
    pub speed_limit_kph: Option<f64>,

    pub lanes: Option<u8>,

    pub name: Option<String>,

    pub traffic_signal_at_end: bool,
}
```

Possible road classes:

```rust
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
```

For v0.1, OSM data that cannot be interpreted safely can simply map to `Other`.

---

## 9. Edge Travel Time

Each edge requires an estimated traversal time:

\[
T(e)=\frac{D(e)}{V(e)}
\]

where:

- \(D(e)\) is edge length;
- \(V(e)\) is estimated driving speed.

Use the explicit OSM speed limit when available.

Otherwise use defaults based on road class.

For example:

```text
Motorway       65 mph
Trunk          55 mph
Primary        45 mph
Secondary      35 mph
Tertiary       30 mph
Residential    25 mph
Service        15 mph
```

These are routing estimates rather than assertions about legal speed limits.

All internal calculations should use SI units.

---

## 10. User Preference Model

Define:

```rust
pub struct DrivingPreferences {
    pub highway_penalty: f64,
    pub minor_road_penalty: f64,

    pub traffic_signal_penalty_s: f64,
    pub left_turn_penalty_s: f64,

    pub max_extra_time_ratio: Option<f64>,
}
```

Example:

```rust
DrivingPreferences {
    highway_penalty: 0.30,
    minor_road_penalty: 0.20,

    traffic_signal_penalty_s: 8.0,
    left_turn_penalty_s: 12.0,

    max_extra_time_ratio: Some(0.15),
}
```

This means the user is willing to tolerate a personalized route up to approximately 15% slower than the fastest route.

---

## 11. Cost Model

The router should express all preference penalties in a common unit:

**equivalent seconds of travel time.**

This makes configuration understandable.

For an edge:

\[
C(e)=T(e)+P(e)
\]

For example, if:

```text
edge traversal time = 60 s

highway penalty = 0.30
```

then:

\[
P_h(e)=0.30\times60=18s
\]

and:

\[
C(e)=78s
\]

Similarly:

```text
traffic light:
    +8 equivalent seconds

left turn:
    +12 equivalent seconds
```

This makes an interpretation possible:

> "I would rather spend approximately 12 additional seconds driving than make this left turn."

That interpretation will later be useful for learning preferences automatically.

---

## 12. Maneuver-Aware Routing

An important design detail is that the cost of entering an edge can depend on the previous edge.

For example:

```text
        previous edge
             │
             ▼
─────────────●──────────────
             │
             │ next edge
             ▼
```

Determining whether the maneuver is:

- straight;
- left;
- right;

requires examining both edges.

Therefore the routing state should conceptually be:

\[
(\text{node},\text{incoming edge})
\]

rather than simply:

\[
\text{node}
\]

The search implementation should therefore allow:

```rust
fn transition_cost(
    graph: &Graph,
    previous: Option<EdgeId>,
    next: EdgeId,
    preferences: &DrivingPreferences,
) -> Cost;
```

This design will later support:

- left-turn penalties;
- U-turn penalties;
- difficult intersection penalties;
- merge penalties;
- lane-change penalties.

---

## 13. Determining Left Turns

Use the geometry of:

```text
previous edge
intersection
next edge
```

to calculate a change in heading.

For example:

```text
-30° ... +30°       straight
+30° ... +150°      right
-150° ... -30°      left
```

The exact thresholds are not important for v0.1 and should be configurable internally.

Care must be taken with driving direction and coordinate-system conventions.

The maneuver classification should have unit tests.

---

## 14. Routing Algorithm

Start with **Dijkstra's algorithm**.

Do not start with contraction hierarchies, bidirectional A*, ALT, or other sophisticated routing optimizations.

For a regional dataset, a basic implementation is enough to validate correctness.

The interface should nevertheless be generic:

```rust
pub trait Router {
    fn route(
        &self,
        graph: &RoadGraph,
        request: &RouteRequest,
    ) -> Result<Route>;
}
```

The initial implementation may be:

```rust
pub struct DijkstraRouter;
```

Later:

```rust
pub struct AStarRouter;
pub struct BidirectionalRouter;
```

can be added without changing higher layers.

---

## 15. Fastest Route vs Personalized Route

Every route request should calculate at least two routes.

### Route A

Optimize only estimated driving time:

\[
C_{\text{fast}}=T
\]

### Route B

Optimize personalized cost:

\[
C_{\text{personal}}=T+P
\]

The comparison between these two routes is an essential part of MyRoute.

The tool should make the trade-off visible instead of simply returning one opaque answer.

---

## 16. Maximum Extra Time

A common user preference is:

> "Avoid things I dislike, but don't make the trip ridiculously longer."

The user may specify:

```text
max_extra_time_ratio = 0.15
```

meaning:

\[
T_{\text{personal}}
\le
1.15T_{\text{fastest}}
\]

A fully optimal constrained shortest-path implementation is unnecessary for the first prototype.

For v0.1, an acceptable implementation is to generate several routes under progressively adjusted preference weights and choose the best personalized candidate satisfying the time constraint.

Alternatively, this feature may initially be advisory:

```text
WARNING:
Personalized route is 22% slower.
Configured limit is 15%.
```

The routing architecture should support a proper constrained implementation later.

---

## 17. Profiles

Users should not need to configure every weight manually.

Profiles should be TOML files.

Example:

```toml
name = "chill"

highway_penalty = 0.20
minor_road_penalty = 0.25

traffic_signal_penalty_s = 10
left_turn_penalty_s = 15

max_extra_time_ratio = 0.15
```

Built-in profiles:

### `fastest`

All penalties zero.

### `chill`

Prefer fewer traffic lights, fewer difficult turns, and somewhat less highway driving.

### `no-highway`

Strongly penalize motorways.

Profile values should remain deliberately subjective rather than being presented as universally good routing rules.

---

## 18. CLI

Primary command:

```bash
myroute route
```

Example:

```bash
myroute route \
    --from "40.3431,-74.6514" \
    --to "40.4862,-74.4518" \
    --profile chill
```

For v0.1, coordinates should be the guaranteed supported input format.

Human-readable place names may be supported through an optional geocoding module, but geocoding should not become part of the routing core.

Useful options:

```text
--from
--to
--profile

--highway-penalty
--minor-road-penalty
--traffic-light-penalty
--left-turn-penalty

--max-extra-time

--output
--format

--verbose
```

---

## 19. Snapping Coordinates to Roads

The requested GPS coordinate will usually not exactly correspond to a graph node.

The program therefore needs:

```rust
fn nearest_road_node(
    graph: &RoadGraph,
    location: Coordinate,
) -> NodeId;
```

A brute-force scan is acceptable for a tiny dataset but should not be the long-term design.

For v0.1, implement a simple spatial index such as an R-tree or k-d tree.

Input:

```text
40.34312,-74.65132
```

Output:

```text
nearest road node:
40.34304,-74.65151

distance:
19.4 m
```

If the nearest routable road is implausibly far away, return an error.

---

## 20. Route Representation

```rust
pub struct Route {
    pub edges: Vec<EdgeId>,

    pub distance_m: f64,
    pub travel_time_s: f64,

    pub personalized_cost: f64,

    pub stats: RouteStats,
}
```

with:

```rust
pub struct RouteStats {
    pub highway_distance_m: f64,
    pub minor_road_distance_m: f64,

    pub traffic_signal_count: usize,
    pub left_turn_count: usize,
}
```

This representation should remain separate from presentation.

---

## 21. Route Explanation

One of MyRoute's distinguishing features should eventually be that it explains its choices.

Even v0.1 should produce basic explanations.

Example:

```text
MyRoute selected a route 3.3 minutes slower than the fastest route.

Compared with the fastest route:

    -6 traffic lights
    -4 left turns
    -6.8 miles of highway
    +1.1 miles total distance
```

Do not attempt natural-language AI explanations in v0.1.

Generate explanations directly from route statistics.

---

## 22. Visualization

The CLI should export the route as **GeoJSON**.

Example:

```bash
myroute route \
    --from ... \
    --to ... \
    --profile chill \
    --output route.geojson
```

The output should contain at least:

```text
fastest route
personalized route
origin
destination
```

A lightweight HTML visualizer can then display the routes on a map.

Recommended structure:

```text
output/
├── route.geojson
└── route.html
```

Opening:

```text
route.html
```

in a browser should display both routes.

For v0.1, using an existing JavaScript map library for rendering is preferable to building any map-rendering infrastructure.

---

## 23. OSM Import Pipeline

Provide a preprocessing command:

```bash
myroute import new-jersey.osm.pbf
```

which generates:

```text
new-jersey.myroute
```

The internal file can initially use a simple serialized Rust representation.

The importer should:

1. parse OSM nodes and ways;
2. retain drivable roads;
3. interpret one-way restrictions;
4. construct directed graph edges;
5. calculate edge distances;
6. classify roads;
7. record traffic signals;
8. serialize the graph.

The routing command then loads the preprocessed graph rather than parsing the PBF on every invocation.

---

## 24. Correctness Requirements

v0.1 does not need production-navigation correctness, but several properties are essential.

The router must:

- respect one-way roads;
- not traverse disconnected graph edges;
- return contiguous routes;
- correctly calculate total route distance;
- correctly calculate route cost;
- return no route when the destination is unreachable;
- avoid integer overflow and invalid floating-point values;
- classify simple left/right/straight turns consistently.

Routing results should be deterministic given identical inputs and graph data.

---

## 25. Testing

### Unit tests

Cover:

```text
edge traversal time
road penalties
traffic-light penalties
left-turn classification
transition costs
route statistics
profile parsing
coordinate distance calculations
```

For example:

```rust
#[test]
fn highway_penalty_increases_cost() {
    // ...
}
```

and:

```rust
#[test]
fn detects_simple_left_turn() {
    // ...
}
```

### Graph tests

Construct tiny synthetic graphs:

```text
A ---- B ---- C
       |
       |
       D
```

and verify exactly which path should be selected under different preferences.

Example:

```text
Route 1:
10 min
3 traffic lights

Route 2:
11 min
0 traffic lights
```

With:

```text
traffic_signal_penalty = 30s
```

Route 2 should win.

Synthetic tests are especially important because they make routing decisions easy to reason about.

### Integration tests

Use several fixed real-world coordinate pairs around Princeton.

Verify that:

- a route exists;
- output is deterministic;
- fastest travel time is no greater than the personalized route's physical travel time in normal cases;
- changing preferences can change the selected route.

---

## 26. Logging and Debugging

Use structured logging with Rust's `tracing` ecosystem.

Running:

```bash
RUST_LOG=myroute=debug myroute route ...
```

should provide information such as:

```text
loaded graph: 483291 nodes, 992301 edges
origin snapped 14.8m from requested point
destination snapped 9.1m from requested point

fastest search:
    explored 18294 states
    cost 1883.1 s

personalized search:
    explored 24192 states
    cost 2204.8
```

This will become particularly useful when experimenting with routing behavior.

---

## 27. Performance Targets

Performance is not the main objective of v0.1.

For a regional dataset, a reasonable initial target is:

```text
graph preprocessing:
    < several minutes

graph loading:
    < 5 seconds

individual route:
    < 2 seconds
```

These are development targets rather than strict requirements.

Do not spend significant effort optimizing until route quality is convincing.

---

## 28. Implementation Stages

### Stage 1 — Synthetic graph router

Implement:

```text
Graph
Node
Edge
Dijkstra
basic travel-time cost
```

Demonstrate:

```text
A → B → C
```

versus:

```text
A → D → C
```

based on different costs.

At this stage there is no OpenStreetMap.

### Stage 2 — Preference-aware routing

Add:

```text
RoadClass
DrivingPreferences
CostModel
highway penalty
minor-road penalty
```

Verify that changing preferences produces different paths on synthetic graphs.

### Stage 3 — Maneuver-aware routing

Add:

```text
incoming edge in routing state
turn-angle calculation
left-turn penalty
traffic-light penalty
```

This establishes the architecture needed for future intersection preferences.

### Stage 4 — OSM importer

Parse a small OSM extract and construct the graph.

Initially use a very small region around Princeton.

Run:

```bash
myroute import princeton.osm.pbf
```

### Stage 5 — Real route

Support:

```bash
myroute route \
    --from 40.3431,-74.6514 \
    --to 40.3573,-74.6672
```

Return a real route.

### Stage 6 — Route comparison

Compute:

```text
fastest route
personalized route
```

and print statistics comparing them.

This stage represents the first true MyRoute prototype.

### Stage 7 — Visualization

Export both routes to GeoJSON and display them in a browser.

At this point v0.1 is complete.

---

## 29. Definition of Done

MyRoute v0.1 is complete when the following demo works:

```bash
myroute import new-jersey.osm.pbf

myroute route \
    --from "40.3431,-74.6514" \
    --to "40.4862,-74.4518" \
    --profile chill \
    --output demo/
```

and produces:

```text
FASTEST

Travel time:      31.2 min
Distance:         24.4 mi
Highway:          18.0 mi
Traffic lights:   14
Left turns:        7


MYROUTE — CHILL

Travel time:      34.5 min
Distance:         25.3 mi
Highway:          10.2 mi
Traffic lights:    7
Left turns:        3


TRADE-OFF

+3.3 min
-7.8 mi highway
-7 traffic lights
-4 left turns

Open:
demo/route.html
```

The generated map should visibly show two different routes.

That demonstration is enough to establish the core thesis of the project:

> **Navigation should optimize for the route the driver wants, not merely the route with the smallest ETA.**

---

## 30. Design Principles for Future Versions

Although these features should not be implemented in v0.1, the architecture should leave room for:

```text
v0.2
interactive web UI

v0.3
preference-learning from route comparisons

v0.4
GPS navigation and rerouting

v0.5
iPhone application

v0.6
CarPlay

later
live traffic
route familiarity
intersection complexity
lane-change difficulty
scenic routing
historical driving behavior
personalized learned cost model
```

The most important long-term abstraction is:

```rust
trait CostModel {
    fn transition_cost(
        &self,
        graph: &RoadGraph,
        previous_edge: Option<EdgeId>,
        next_edge: EdgeId,
    ) -> Cost;
}
```

Everything from a simple `chill.toml` profile to a future machine-learned model should eventually implement this abstraction.

---

## 31. Recommended First Coding Session

Do **not** begin by downloading OSM data.

Create:

```text
myroute-core
```

and implement a six-node fictional city:

```text
              Highway
        ┌───────────────────┐
        │                   │
        A                   D
        │                   │
        B──────C──────E─────F
           local streets
```

Make the highway route:

```text
20 minutes
```

and the local route:

```text
23 minutes
```

Then make:

```bash
cargo run -- --profile fastest
```

choose the highway, while:

```bash
cargo run -- --profile no-highway
```

chooses the local road.

Once that works, you already have the central mechanism of MyRoute.

Everything after that is progressively replacing your imaginary city with the real world.
