# MyRoute v0.1 Implementation Plan

## Summary

Implement MyRoute as a Rust workspace with separate core-routing, OSM-import, and CLI crates. Version 0.1 imports a regional OpenStreetMap PBF extract, accepts coordinate endpoints, computes fastest and preference-aware routes, enforces the maximum-extra-time constraint through deterministic preference scaling, compares the results, and exports GeoJSON plus an HTML map.

Human-readable geocoding, turn-restriction relations, live traffic, mobile applications, and navigation guidance remain outside v0.1.

## Technical Design

### Workspace and contracts

- `myroute-core` owns dense graph types, spatial lookup, geometry, profiles, transition-aware costs, deterministic Dijkstra routing, constrained route selection, route analysis, and typed errors.
- `myroute-osm` owns PBF tag interpretation, directed graph construction, a validated and versioned `.myroute` binary format, and graph loading.
- `myroute-cli` owns `import` and `route`, profile/flag precedence, diagnostics, comparison output, GeoJSON, HTML generation, and logging.
- Core remains synchronous and independent from OSM parsing and presentation. Public IDs use dense `u32` newtypes, calculations use SI units, and all input numeric values are validated as finite and nonnegative where applicable.

### Routing behavior

- Build directed adjacency lists and an R-tree over routable nodes. Reject corrupt references and snap requests farther than 1 km from the road graph.
- Estimate edge time from a valid OSM speed or documented road-class defaults. Treat Motorway and Trunk as highway exposure, and Residential, Service, and Other as minor-road exposure.
- Express every preference as equivalent seconds. Road-class penalties multiply physical edge time; traffic-signal and left-turn penalties are fixed transition costs.
- Search state is `(node, incoming_edge)`, allowing the cost model to classify maneuvers from adjacent edge geometry. Stable state and edge IDs break equal-cost ties deterministically.
- Calculate the physical-time fastest route first. Search personalized candidates at penalty scales `1.0`, `0.75`, `0.5`, `0.25`, and `0.0`; remove duplicate paths and over-budget candidates, then minimize full unscaled personalized cost, physical time, and edge IDs in that order. Scale zero guarantees a feasible fastest-route fallback.

### Import, CLI, and output

- Import common drivable OSM highway classes, exclude explicitly non-drivable/private access, honor forward/reverse one-way values and implicit roundabout direction, parse straightforward `km/h`/`mph` speeds, preserve signal nodes and way geometry, and report retained/skipped counts.
- Serialize a graph envelope with magic bytes and schema version 1. Reject wrong versions, truncated files, invalid values, broken references, and empty graphs.
- Guarantee `LAT,LON` inputs. Reject malformed or out-of-range coordinates and explain that place-name geocoding is not supported in v0.1.
- Ship `fastest`, `chill`, and `no-highway` profiles. Start from `fastest` when no profile is supplied; individual CLI options override selected profile fields.
- Print fastest and personalized statistics plus deterministic difference-based explanations. Define preference score as full personalized cost divided by the fastest route's full personalized cost, with lower being better and the fastest baseline displayed as `1.00`.
- Write `route.geojson` with both lines, requested endpoints, and snapped endpoints. Write self-contained route data into a Leaflet HTML viewer; map assets and tiles may require internet access.

## Implementation Sequence and Agent Workflow

1. The integration lead freezes public interfaces, owns workspace/shared files, and gives each specialist nonoverlapping paths and acceptance tests.
2. The core specialist implements graph, geometry, costs, profiles, Dijkstra, constrained selection, and synthetic tests.
3. After core contracts compile, the OSM specialist implements import, graph persistence, and fixture tests while the CLI specialist implements command parsing, output, visualization, and subprocess tests.
4. Every specialist reads `AGENTS.md`, reports changed paths and commands, runs focused tests plus `cargo +nightly fmt --all`, and never commits, pushes, branches, or deletes.
5. A read-only verification pass checks correctness, determinism, numeric validation, OSM semantics, panic risks, acceptance coverage, and scope. Blocking findings return to the owning specialist.
6. The lead runs workspace-wide gates and owns changes to shared manifests or frozen interfaces. Interface changes stop dependent work until the new contract and affected tests are approved.

## Test and Release Plan

- Unit tests cover distances, speed fallback, penalties, invalid floats, maneuver boundaries, profiles, scores, and preference scaling.
- Synthetic graph tests cover fastest/personalized choices, signals, left turns, one-way edges, cycles, parallel edges, disconnected nodes, contiguity, stable ties, and constrained fallback.
- Import tests cover road filtering/classification, access, speed units, one-way variants, roundabouts, signal propagation, serialization round trips, corrupt versions, and invalid references.
- CLI tests cover successful import/route flows, precedence, malformed coordinates, unsupported names, missing/corrupt graphs, snapping limits, no-route errors, deterministic summaries, and output artifacts.
- Release gates are `cargo +nightly fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and `cargo build --release`, with no unapproved `#[allow(...)]` attributes.
- Real-data acceptance imports a Princeton/New Jersey extract and routes from `40.3431,-74.6514` to `40.4862,-74.4518`; both routes must be contiguous and deterministic, the personalized route must meet its time budget, at least one fixed case must change under a preference profile, and both map artifacts must render the routes and endpoints.

## Assumptions and Defaults

- Source data is user-supplied and is not downloaded automatically; regional PBF and generated graph files stay out of version control.
- Coordinate inputs are the only guaranteed endpoint format.
- Turn-restriction relations and complex conditional tags are documented v0.1 limitations.
- `.myroute` compatibility is guaranteed only for schema version 1.
- Terminal display uses miles and minutes while core calculations remain SI.
- Correctness and explainability take priority over the specification's development performance targets.
