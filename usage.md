# MyRoute v0.1 Usage

MyRoute is a command-line routing prototype that compares the fastest driving route with a route adjusted for preferences such as less highway driving, fewer minor roads, fewer traffic signals, and fewer left turns.

## Prerequisites

- Rust stable and Cargo.
- A small regional OpenStreetMap `.osm.pbf` extract covering the desired route.
- Internet access during the first build so Cargo can download dependencies.
- Internet access when viewing `route.html` if you want the hosted Leaflet assets and OpenStreetMap basemap tiles to load.

MyRoute v0.1 is intended for regional extracts, such as Princeton or New Jersey, rather than planet-scale data. Map extracts are not downloaded automatically.

## Build

From the repository root:

```bash
cargo build --release
```

The executable is created at:

```text
target/release/myroute
```

The examples below use that path. You can substitute `myroute` if you copy the executable into a directory on your `PATH`.

Confirm the installation with:

```bash
target/release/myroute --help
```

## Quick start

First, import a regional PBF extract into MyRoute's graph format:

```bash
target/release/myroute import \
  data/new-jersey.osm.pbf \
  --output data/new-jersey.myroute
```

Then compare routes between two coordinates:

```bash
target/release/myroute route \
  --from "40.3431,-74.6514" \
  --to "40.4862,-74.4518" \
  --graph data/new-jersey.myroute \
  --profile chill \
  --output demo/
```

The command prints fastest and personalized route statistics. It also creates:

```text
demo/
├── route.geojson
└── route.html
```

Open `demo/route.html` in a browser to view both routes. `route.geojson` can also be opened in a compatible GIS or map viewer.

## Import map data

Command syntax:

```text
myroute import [OPTIONS] <INPUT>
```

Basic import:

```bash
target/release/myroute import new-jersey.osm.pbf
```

For an input named `new-jersey.osm.pbf`, the default output is `new-jersey.myroute` in the same directory. Set an explicit destination with `--output`:

```bash
target/release/myroute import \
  data/princeton.osm.pbf \
  --output data/princeton.myroute
```

The importer reports scanned, retained, skipped, and invalid way counts, turn-restriction counts, and the number of graph nodes and directed edges created. It retains supported drivable roads, interprets common one-way and roundabout tags, imports via-node motor-vehicle turn restrictions, records directional traffic signals and basic road attributes, and builds a graph optimized for repeated route requests.

Available import options:

- `-o, --output <OUTPUT>`: destination `.myroute` file.
- `-v, --verbose`: enable diagnostic logging.

Source `.osm.pbf` files and generated `.myroute` files are ignored by Git in the provided `data/` directory.

## Calculate and compare routes

Command syntax:

```text
myroute route [OPTIONS] --from <LAT,LON> --to <LAT,LON>
```

Coordinates are the guaranteed endpoint format in v0.1. Latitude comes first:

```bash
target/release/myroute route \
  --from "40.3431,-74.6514" \
  --to "40.3573,-74.6672" \
  --graph data/princeton.myroute
```

Place names such as `Princeton, NJ` are not supported. Each coordinate is snapped to the nearest routable graph node. MyRoute returns an error if that node is more than 1 km away, which usually means the selected graph does not cover the requested location.

If `--graph` is omitted, MyRoute looks for `new-jersey.myroute` in the current directory.

### Built-in profiles

Select a built-in profile with `--profile`:

- `fastest`: disables preference penalties and minimizes estimated driving time.
- `chill`: moderately avoids highways, minor roads, difficult turns, and traffic signals, with a 15% extra-time limit.
- `no-highway`: strongly penalizes highway travel and U-turns, with a 25% extra-time limit.

Example:

```bash
target/release/myroute route \
  --from "40.3431,-74.6514" \
  --to "40.4862,-74.4518" \
  --graph data/new-jersey.myroute \
  --profile no-highway
```

The default profile is `fastest`.

### Custom profiles

`--profile` also accepts a path to a TOML file:

```toml
name = "my-profile"
highway_penalty = 0.30
minor_road_penalty = 0.20
traffic_signal_penalty_s = 8.0
left_turn_penalty_s = 12.0
turn_penalty_s = 5.0
u_turn_penalty_s = 120.0
max_extra_time_ratio = 0.15
```

Use it with:

```bash
target/release/myroute route \
  --from "40.3431,-74.6514" \
  --to "40.4862,-74.4518" \
  --graph data/new-jersey.myroute \
  --profile profiles/my-profile.toml
```

All penalties must be finite and nonnegative. `max_extra_time_ratio` is optional.

### Command-line preference overrides

Individual options override the corresponding selected-profile value:

```bash
target/release/myroute route \
  --from "40.3431,-74.6514" \
  --to "40.4862,-74.4518" \
  --graph data/new-jersey.myroute \
  --profile chill \
  --highway-penalty 0.60 \
  --minor-road-penalty 0.80 \
  --traffic-light-penalty 15 \
  --left-turn-penalty 20 \
  --turn-penalty 5 \
  --u-turn-penalty 120 \
  --max-extra-time 0.15
```

The options mean:

- `--highway-penalty <RATIO>`: adds a fraction of highway travel time as equivalent cost. A value of `0.60` adds 60% of the edge's travel time.
- `--minor-road-penalty <RATIO>`: applies the same model to minor roads.
- `--traffic-light-penalty <SECONDS>`: adds equivalent seconds for each physical traffic signal.
- `--left-turn-penalty <SECONDS>`: adds equivalent seconds for each unsignalized left turn. Signal-controlled lefts do not receive this extra penalty.
- `--turn-penalty <SECONDS>`: adds equivalent seconds for each ordinary left or right turn.
- `--u-turn-penalty <SECONDS>`: adds equivalent seconds for U-turn transitions and U-turn-shaped connector edges.
- `--max-extra-time <RATIO>`: limits physical travel time above the fastest route. A value of `0.15` means 15%.

When the fully weighted route exceeds the time limit, MyRoute evaluates progressively reduced penalty scales and chooses the best feasible candidate. The fastest route is the final fallback, so a configured time limit is always respected when a route exists.

### Route output

Terminal output includes:

- Estimated time and distance for the fastest and personalized routes.
- Highway and minor-road distance.
- Traffic-signal, signalized-left, left-turn, right-turn, and U-turn counts.
- Time and distance differences.
- A deterministic explanation of the trade-off.
- Preference scores, where lower is better and the fastest-route baseline is shown as `1.00`.

Write visualization files by passing an output directory:

```bash
target/release/myroute route \
  --from "40.3431,-74.6514" \
  --to "40.4862,-74.4518" \
  --graph data/new-jersey.myroute \
  --profile chill \
  --output demo/ \
  --format geojson
```

The GeoJSON contains fastest and personalized route features plus requested and snapped endpoints. Route data is embedded in the HTML file, so the browser does not need to load `route.geojson` separately.

## Diagnostics

Use `--verbose` for import or route diagnostics:

```bash
target/release/myroute route \
  --from "40.3431,-74.6514" \
  --to "40.4862,-74.4518" \
  --graph data/new-jersey.myroute \
  --profile chill \
  --verbose
```

You can also control structured logging with `RUST_LOG`:

```bash
RUST_LOG=myroute_core=debug,myroute_osm=debug,myroute_cli=debug \
  target/release/myroute route \
  --from "40.3431,-74.6514" \
  --to "40.4862,-74.4518" \
  --graph data/new-jersey.myroute
```

Common failures include:

- **Place-name error:** use numeric `LAT,LON` coordinates.
- **Nearest road is too far away:** use a graph that covers both coordinates or choose points nearer imported roads.
- **No route:** the snapped nodes are disconnected or one-way roads prevent reaching the destination.
- **Missing or corrupt graph:** run `myroute import` again and pass the resulting file through `--graph`.
- **Unsupported graph version:** regenerate the graph using the same MyRoute version as the routing executable.
- **Blank HTML basemap:** connect to the internet so Leaflet and OpenStreetMap tiles can load; route coordinates remain available in `route.geojson`.

## Development checks

Install a nightly toolchain if it is not already available, then run:

```bash
cargo +nightly fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release
```

## v0.1 limitations

- No human-readable geocoding.
- No OSM turn-restriction relations or complex conditional access and speed rules.
- No real-time traffic, rerouting, GPS tracking, or turn-by-turn guidance.
- No mobile, iPhone, or CarPlay application.
- Regional datasets only.
- Estimated travel time is based on OSM speed tags or road-class defaults and is not a legal-speed or arrival-time guarantee.

OpenStreetMap data is © OpenStreetMap contributors and is available under the Open Database License. Preserve the required attribution when distributing map output or derived data.
