# MyRoute

MyRoute v0.1 is a desktop routing prototype that compares the fastest driving route with a route optimized for user preferences such as less highway exposure, fewer minor roads, fewer traffic signals, and fewer left turns.

## Build

```bash
cargo build --release
```

The executable is written to `target/release/myroute`.

## Import map data

Download a small regional OpenStreetMap `.osm.pbf` extract yourself, then preprocess it:

```bash
myroute import new-jersey.osm.pbf
```

This writes `new-jersey.myroute` by default. Large source extracts and generated graph files are intentionally ignored by Git. See `data/README.md` for data and attribution details.

## Compare routes

Coordinates are the guaranteed v0.1 endpoint format:

```bash
myroute route \
  --from "40.3431,-74.6514" \
  --to "40.4862,-74.4518" \
  --profile chill \
  --output demo/
```

The command prints the fastest and personalized route statistics. When an output directory is supplied, it also writes `route.geojson` and `route.html`.

Use `myroute route --help` for individual preference overrides and graph selection. Human-readable place-name geocoding is not supported in v0.1.

## Development

```bash
cargo +nightly fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release
```

The design specification is in `specs/myroute_v0.1_spec.md`; the delivery plan is in `specs/myroute_v0.1_implementation_plan.md`.

## Known v0.1 limitations

- Regional extracts only; performance is not intended for planet-scale graphs.
- OSM turn-restriction relations and complex conditional access/speed tags are not interpreted.
- Estimated travel time does not include live traffic.
- The generated HTML uses hosted Leaflet assets and OpenStreetMap tiles, so its basemap requires internet access.

## License

MIT. Map data remains subject to the OpenStreetMap contributors' ODbL terms.
