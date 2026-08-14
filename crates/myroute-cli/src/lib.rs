mod output;

use std::fs;
use std::path::{Path, PathBuf};

use clap::{Args, Parser, Subcommand, ValueEnum};
use myroute_core::{
    Coordinate, DijkstraRouter, DrivingPreferences, DrivingProfile, PersonalizedCost, RoadGraph,
    Route, RouteRequest,
};
use thiserror::Error;
use tracing::{debug, info};
use tracing_subscriber::EnvFilter;

const MAX_SNAP_DISTANCE_M: f64 = 1_000.0;
const FASTEST_PROFILE: &str = include_str!("../../../profiles/fastest.toml");
const CHILL_PROFILE: &str = include_str!("../../../profiles/chill.toml");
const NO_HIGHWAY_PROFILE: &str = include_str!("../../../profiles/no-highway.toml");

#[derive(Debug, Error)]
pub enum CliError {
    #[error("could not {action} `{path}`: {source}")]
    Io {
        action: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid driving profile `{profile}`: {message}")]
    Profile { profile: String, message: String },
    #[error("{0}")]
    Core(#[from] myroute_core::Error),
    #[error("OpenStreetMap operation failed: {0}")]
    Osm(String),
    #[error("could not encode route output: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Parser)]
#[command(name = "myroute", version, about = "Preference-aware driving routes")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Import an OpenStreetMap PBF extract into a .myroute graph.
    Import(ImportArgs),
    /// Compare the fastest and preference-aware routes.
    Route(RouteArgs),
}

#[derive(Debug, Args)]
struct ImportArgs {
    /// Input .osm.pbf file.
    input: PathBuf,
    /// Destination .myroute graph (defaults to the input name).
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// Enable diagnostic logging.
    #[arg(short, long)]
    verbose: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum OutputFormat {
    Geojson,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Location(Coordinate);

#[derive(Debug, Args)]
struct RouteArgs {
    /// Origin as LAT,LON (place-name geocoding is not supported in v0.1).
    #[arg(long, value_parser = parse_location)]
    from: Location,
    /// Destination as LAT,LON (place-name geocoding is not supported in v0.1).
    #[arg(long, value_parser = parse_location)]
    to: Location,
    /// Imported graph file.
    #[arg(long, default_value = "new-jersey.myroute")]
    graph: PathBuf,
    /// Built-in profile name or path to a TOML profile.
    #[arg(long, default_value = "fastest")]
    profile: String,
    /// Extra equivalent-time cost per unit of highway travel time.
    #[arg(long, value_parser = parse_nonnegative)]
    highway_penalty: Option<f64>,
    /// Extra equivalent-time cost per unit of minor-road travel time.
    #[arg(long, value_parser = parse_nonnegative)]
    minor_road_penalty: Option<f64>,
    /// Extra seconds charged for each traffic signal.
    #[arg(long, value_parser = parse_nonnegative)]
    traffic_light_penalty: Option<f64>,
    /// Extra seconds charged for each left turn.
    #[arg(long, value_parser = parse_nonnegative)]
    left_turn_penalty: Option<f64>,
    /// Extra seconds charged for each non-straight left or right turn.
    #[arg(long, value_parser = parse_nonnegative)]
    turn_penalty: Option<f64>,
    /// Extra seconds charged for each U-turn-like movement.
    #[arg(long, value_parser = parse_nonnegative)]
    u_turn_penalty: Option<f64>,
    /// Maximum personalized travel-time ratio above the fastest route.
    #[arg(long, value_parser = parse_nonnegative)]
    max_extra_time: Option<f64>,
    /// Directory for route.geojson and route.html.
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// Route interchange format (v0.1 supports GeoJSON).
    #[arg(long, value_enum, default_value_t = OutputFormat::Geojson)]
    format: OutputFormat,
    /// Enable diagnostic logging.
    #[arg(short, long)]
    verbose: bool,
}

pub fn run_from_env() -> Result<(), CliError> {
    run(Cli::parse())
}

fn run(cli: Cli) -> Result<(), CliError> {
    let verbose = match &cli.command {
        Command::Import(args) => args.verbose,
        Command::Route(args) => args.verbose,
    };
    init_logging(verbose);
    match cli.command {
        Command::Import(args) => run_import(args),
        Command::Route(args) => run_route(args),
    }
}

fn init_logging(verbose: bool) {
    let fallback = if verbose { "debug" } else { "warn" };
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(fallback));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .try_init();
}

fn run_import(args: ImportArgs) -> Result<(), CliError> {
    let output = args
        .output
        .unwrap_or_else(|| default_graph_path(&args.input));
    info!(input = %args.input.display(), "importing OpenStreetMap data");
    let (graph, report) = import_graph(&args.input)?;
    debug!(
        nodes = graph.node_count(),
        edges = graph.edge_count(),
        "graph built"
    );
    save_graph(&output, &graph)?;
    println!(
        "Parsed {} nodes, {} ways, and {} relations\n\
         Retained {} ways and {} nodes; skipped {} ways; {} invalid ways\n\
         Created {} directed edges and {} forbidden transitions\n\
         Turn restrictions: {} found, {} applied, {} unsupported, {} motorcar-exempt\n\
         Wrote {}",
        report.parsed_nodes,
        report.parsed_ways,
        report.parsed_relations,
        report.retained_ways,
        report.retained_nodes,
        report.skipped_ways,
        report.invalid_ways,
        report.created_edges,
        report.created_turn_restrictions,
        report.restriction_relations,
        report.applied_restriction_relations,
        report.unsupported_restriction_relations,
        report.exempt_restriction_relations,
        output.display()
    );
    Ok(())
}

fn run_route(args: RouteArgs) -> Result<(), CliError> {
    let preferences = resolve_preferences(&args)?;
    let graph = load_graph(&args.graph)?;
    info!(
        nodes = graph.node_count(),
        edges = graph.edge_count(),
        "graph loaded"
    );

    let from = graph.nearest_road_node_with_limit(args.from.0, MAX_SNAP_DISTANCE_M)?;
    let to = graph.nearest_road_node_with_limit(args.to.0, MAX_SNAP_DISTANCE_M)?;
    debug!(
        distance_m = from.distance_m,
        node = from.node.0,
        "origin snapped"
    );
    debug!(
        distance_m = to.distance_m,
        node = to.node.0,
        "destination snapped"
    );

    let request = RouteRequest::new(from.node, to.node, preferences);
    let comparison = DijkstraRouter.route_comparison(&graph, &request)?;
    log_route_transitions("fastest", &graph, &comparison.fastest, &preferences)?;
    log_route_transitions(
        "personalized",
        &graph,
        &comparison.personalized,
        &preferences,
    )?;
    println!(
        "{}",
        output::terminal_summary(args.from.0, args.to.0, &comparison)
    );

    if let Some(directory) = args.output {
        match args.format {
            OutputFormat::Geojson => output::write_map_outputs(
                &directory,
                &graph,
                args.from.0,
                args.to.0,
                from.coordinate,
                to.coordinate,
                &comparison,
            )?,
        }
        println!("\nWrote {}", directory.join("route.geojson").display());
        println!("Wrote {}", directory.join("route.html").display());
    }
    Ok(())
}

fn default_graph_path(input: &Path) -> PathBuf {
    if let Some(stem) = input
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_suffix(".osm.pbf"))
    {
        return input.with_file_name(format!("{stem}.myroute"));
    }
    let mut output = input.to_path_buf();
    output.set_extension("myroute");
    output
}

fn parse_location(input: &str) -> Result<Location, String> {
    if input.chars().any(char::is_alphabetic) {
        return Err("place-name geocoding is not supported in v0.1; use LAT,LON".into());
    }
    let Some((lat, lon)) = input.split_once(',') else {
        return Err("expected LAT,LON, for example 40.3431,-74.6514".into());
    };
    if lon.contains(',') {
        return Err("expected exactly two values in LAT,LON order".into());
    }
    let lat = lat
        .trim()
        .parse::<f64>()
        .map_err(|_| "latitude is not a number; expected LAT,LON".to_owned())?;
    let lon = lon
        .trim()
        .parse::<f64>()
        .map_err(|_| "longitude is not a number; expected LAT,LON".to_owned())?;
    Coordinate::new(lat, lon)
        .map(Location)
        .map_err(|error| error.to_string())
}

fn parse_nonnegative(input: &str) -> Result<f64, String> {
    let value = input
        .parse::<f64>()
        .map_err(|_| format!("`{input}` is not a number"))?;
    if value.is_finite() && value >= 0.0 {
        Ok(value)
    } else {
        Err("value must be finite and nonnegative".into())
    }
}

fn resolve_preferences(args: &RouteArgs) -> Result<DrivingPreferences, CliError> {
    let profile_text = match args.profile.as_str() {
        "fastest" => FASTEST_PROFILE.to_owned(),
        "chill" => CHILL_PROFILE.to_owned(),
        "no-highway" => NO_HIGHWAY_PROFILE.to_owned(),
        path => fs::read_to_string(path).map_err(|source| CliError::Io {
            action: "read driving profile",
            path: PathBuf::from(path),
            source,
        })?,
    };
    let mut preferences = DrivingProfile::from_toml(&profile_text)
        .map_err(|error| CliError::Profile {
            profile: args.profile.clone(),
            message: error.to_string(),
        })?
        .preferences;
    if let Some(value) = args.highway_penalty {
        preferences.highway_penalty = value;
    }
    if let Some(value) = args.minor_road_penalty {
        preferences.minor_road_penalty = value;
    }
    if let Some(value) = args.traffic_light_penalty {
        preferences.traffic_signal_penalty_s = value;
    }
    if let Some(value) = args.left_turn_penalty {
        preferences.left_turn_penalty_s = value;
    }
    if let Some(value) = args.turn_penalty {
        preferences.turn_penalty_s = value;
    }
    if let Some(value) = args.u_turn_penalty {
        preferences.u_turn_penalty_s = value;
    }
    if let Some(value) = args.max_extra_time {
        preferences.max_extra_time_ratio = Some(value);
    }
    preferences.validate()?;
    Ok(preferences)
}

fn log_route_transitions(
    label: &str,
    graph: &RoadGraph,
    route: &Route,
    preferences: &DrivingPreferences,
) -> Result<(), CliError> {
    let model = PersonalizedCost::new(*preferences)?;
    let mut previous = None;
    for &next in &route.edges {
        let edge = graph.edge(next).ok_or(myroute_core::Error::InvalidRoute(
            "route references a missing edge",
        ))?;
        let evaluation = model.evaluate_transition(graph, previous, next)?;
        let from = graph
            .node(edge.from)
            .ok_or(myroute_core::Error::NodeNotFound(edge.from))?
            .coordinate;
        let to = graph
            .node(edge.to)
            .ok_or(myroute_core::Error::NodeNotFound(edge.to))?
            .coordinate;
        debug!(
            route = label,
            edge_id = next.0,
            previous_edge_id = ?previous.map(|id: myroute_core::EdgeId| id.0),
            source_way_id = ?edge.source_way_id,
            road_name = ?edge.name,
            from_lat = from.lat,
            from_lon = from.lon,
            to_lat = to.lat,
            to_lon = to.lon,
            maneuver = ?evaluation.maneuver,
            transition_angle_deg = ?evaluation.transition_angle_deg,
            edge_heading_change_deg = evaluation.edge_heading_change_deg,
            signal_controlled = evaluation.signal_controlled,
            traffic_signal = evaluation.traffic_signal,
            u_turn_like = evaluation.u_turn_like,
            physical_time_s = evaluation.physical_time_s,
            highway_penalty_s = evaluation.highway_penalty_s,
            minor_road_penalty_s = evaluation.minor_road_penalty_s,
            traffic_signal_penalty_s = evaluation.traffic_signal_penalty_s,
            turn_penalty_s = evaluation.turn_penalty_s,
            left_turn_penalty_s = evaluation.left_turn_penalty_s,
            u_turn_penalty_s = evaluation.u_turn_penalty_s,
            total_cost_s = evaluation.total_cost_s,
            "route transition"
        );
        previous = Some(next);
    }
    Ok(())
}

fn import_graph(path: &Path) -> Result<(RoadGraph, myroute_osm::ImportReport), CliError> {
    myroute_osm::import_pbf(path).map_err(|error| CliError::Osm(error.to_string()))
}

fn load_graph(path: &Path) -> Result<RoadGraph, CliError> {
    myroute_osm::load_graph(path).map_err(|error| CliError::Osm(error.to_string()))
}

fn save_graph(path: &Path, graph: &RoadGraph) -> Result<(), CliError> {
    myroute_osm::save_graph(path, graph).map_err(|error| CliError::Osm(error.to_string()))
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[test]
    fn parses_coordinates_and_explains_place_names() {
        assert_eq!(
            parse_location("40.3431, -74.6514").unwrap().0,
            Coordinate::new(40.3431, -74.6514).unwrap()
        );
        assert!(
            parse_location("Princeton, NJ")
                .unwrap_err()
                .contains("geocoding")
        );
        assert!(parse_location("91,0").unwrap_err().contains("latitude"));
        assert!(parse_location("0,181").unwrap_err().contains("longitude"));
        assert!(parse_location("0,1,2").is_err());
    }

    #[test]
    fn rejects_invalid_penalties_during_argument_parsing() {
        for value in ["-1", "NaN", "inf"] {
            let result = Cli::try_parse_from([
                "myroute",
                "route",
                "--from",
                "0,0",
                "--to",
                "0,1",
                "--highway-penalty",
                value,
            ]);
            assert!(result.is_err(), "accepted {value}");
        }
    }

    #[test]
    fn cli_overrides_selected_profile_fields() {
        let cli = Cli::try_parse_from([
            "myroute",
            "route",
            "--from",
            "0,0",
            "--to",
            "0,1",
            "--profile",
            "chill",
            "--highway-penalty",
            "0.9",
            "--turn-penalty",
            "7",
            "--u-turn-penalty",
            "90",
            "--max-extra-time",
            "0.2",
        ])
        .unwrap();
        let Command::Route(args) = cli.command else {
            panic!("expected route command")
        };
        let preferences = resolve_preferences(&args).unwrap();
        assert_eq!(preferences.highway_penalty, 0.9);
        assert_eq!(preferences.minor_road_penalty, 0.25);
        assert_eq!(preferences.turn_penalty_s, 7.0);
        assert_eq!(preferences.u_turn_penalty_s, 90.0);
        assert_eq!(preferences.max_extra_time_ratio, Some(0.2));
    }

    #[test]
    fn import_output_replaces_final_extension() {
        assert_eq!(
            default_graph_path(Path::new("map.osm.pbf")),
            PathBuf::from("map.myroute")
        );
    }
}
