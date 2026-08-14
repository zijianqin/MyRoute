use std::fs::File;
use std::path::Path;

use myroute_core::RoadGraph;
use osmpbfreader::{OsmObj, OsmPbfReader};

use crate::builder::{build_graph, is_drivable_way, is_turn_restriction_relation};
use crate::{Error, Result};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ImportReport {
    pub parsed_nodes: usize,
    pub parsed_ways: usize,
    pub retained_ways: usize,
    pub skipped_ways: usize,
    pub invalid_ways: usize,
    pub retained_nodes: usize,
    pub created_edges: usize,
    pub parsed_relations: usize,
    pub restriction_relations: usize,
    pub applied_restriction_relations: usize,
    pub unsupported_restriction_relations: usize,
    pub exempt_restriction_relations: usize,
    pub created_turn_restrictions: usize,
}

pub fn import_pbf(input: &Path) -> Result<(RoadGraph, ImportReport)> {
    let file = File::open(input).map_err(|source| Error::OpenPbf {
        path: input.to_path_buf(),
        source,
    })?;
    let mut report = ImportReport::default();
    let mut reader = OsmPbfReader::new(file);
    let objects = reader
        .get_objs_and_deps(|object| match object {
            OsmObj::Node(_) => {
                report.parsed_nodes += 1;
                false
            }
            OsmObj::Way(way) => {
                report.parsed_ways += 1;
                let retain = is_drivable_way(way);
                if !retain {
                    report.skipped_ways += 1;
                }
                retain
            }
            OsmObj::Relation(relation) => {
                report.parsed_relations += 1;
                let retain = is_turn_restriction_relation(relation);
                if retain {
                    report.restriction_relations += 1;
                }
                retain
            }
        })
        .map_err(|source| Error::ParsePbf {
            path: input.to_path_buf(),
            source,
        })?;
    let graph = build_graph(&objects, &mut report)?;
    tracing::info!(
        parsed_nodes = report.parsed_nodes,
        parsed_ways = report.parsed_ways,
        retained_ways = report.retained_ways,
        skipped_ways = report.skipped_ways,
        invalid_ways = report.invalid_ways,
        retained_nodes = report.retained_nodes,
        created_edges = report.created_edges,
        restriction_relations = report.restriction_relations,
        applied_restriction_relations = report.applied_restriction_relations,
        unsupported_restriction_relations = report.unsupported_restriction_relations,
        exempt_restriction_relations = report.exempt_restriction_relations,
        created_turn_restrictions = report.created_turn_restrictions,
        "imported OSM road graph"
    );
    Ok((graph, report))
}
