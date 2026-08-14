use std::fs;
use std::path::Path;

use myroute_core::RoadGraph;
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

pub const GRAPH_MAGIC: [u8; 8] = *b"MYROUTE\0";
pub const GRAPH_SCHEMA_VERSION: u16 = 2;
/// Maximum encoded size supported for a v0.1 regional graph (2 GiB).
pub const MAX_GRAPH_FILE_SIZE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const BINCODE_LIMIT_BYTES: usize = MAX_GRAPH_FILE_SIZE_BYTES as usize;

#[derive(Serialize, Deserialize)]
struct GraphEnvelope {
    magic: [u8; 8],
    schema_version: u16,
    graph: RoadGraph,
}

#[derive(Serialize)]
struct GraphEnvelopeRef<'a> {
    magic: [u8; 8],
    schema_version: u16,
    graph: &'a RoadGraph,
}

#[derive(Deserialize)]
struct GraphHeader {
    magic: [u8; 8],
    schema_version: u16,
}

pub fn save_graph(path: &Path, graph: &RoadGraph) -> Result<()> {
    let envelope = GraphEnvelopeRef {
        magic: GRAPH_MAGIC,
        schema_version: GRAPH_SCHEMA_VERSION,
        graph,
    };
    let config = bincode::config::standard().with_limit::<BINCODE_LIMIT_BYTES>();
    let encoded_size =
        bincode::serde::encode_into_std_write(&envelope, &mut std::io::sink(), config)?;
    validate_graph_size(encoded_size as u64)?;
    let bytes = bincode::serde::encode_to_vec(envelope, config)?;
    validate_graph_size(bytes.len() as u64)?;
    fs::write(path, bytes).map_err(|source| Error::WriteGraph {
        path: path.to_path_buf(),
        source,
    })
}

pub fn load_graph(path: &Path) -> Result<RoadGraph> {
    let metadata = fs::metadata(path).map_err(|source| Error::ReadGraph {
        path: path.to_path_buf(),
        source,
    })?;
    validate_graph_size(metadata.len())?;
    let bytes = fs::read(path).map_err(|source| Error::ReadGraph {
        path: path.to_path_buf(),
        source,
    })?;
    validate_graph_size(bytes.len() as u64)?;
    decode_graph(&bytes)
}

fn decode_graph(bytes: &[u8]) -> Result<RoadGraph> {
    decode_graph_with_limit::<BINCODE_LIMIT_BYTES>(bytes)
}

fn decode_graph_with_limit<const LIMIT: usize>(bytes: &[u8]) -> Result<RoadGraph> {
    let (header, _): (GraphHeader, usize) = bincode::serde::decode_from_slice(
        bytes,
        bincode::config::standard().with_limit::<LIMIT>(),
    )?;
    validate_header(&header)?;
    let (envelope, consumed): (GraphEnvelope, usize) = bincode::serde::decode_from_slice(
        bytes,
        bincode::config::standard().with_limit::<LIMIT>(),
    )?;
    if consumed != bytes.len() {
        return Err(Error::TrailingData(bytes.len() - consumed));
    }
    Ok(envelope.graph)
}

fn validate_header(header: &GraphHeader) -> Result<()> {
    if header.magic != GRAPH_MAGIC {
        return Err(Error::InvalidMagic {
            expected: GRAPH_MAGIC,
            actual: header.magic,
        });
    }
    if header.schema_version != GRAPH_SCHEMA_VERSION {
        return Err(Error::UnsupportedSchemaVersion {
            actual: header.schema_version,
            supported: GRAPH_SCHEMA_VERSION,
        });
    }
    Ok(())
}

fn validate_graph_size(size: u64) -> Result<()> {
    if size > MAX_GRAPH_FILE_SIZE_BYTES {
        Err(Error::GraphTooLarge {
            size,
            max: MAX_GRAPH_FILE_SIZE_BYTES,
        })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use myroute_core::{Coordinate, Edge, EdgeId, Node, NodeId, RoadClass, TurnRestriction};

    use super::*;

    #[test]
    fn graph_round_trip_rebuilds_derived_indexes() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("graph.myroute");
        let graph = graph();
        save_graph(&path, &graph).unwrap();
        let loaded = load_graph(&path).unwrap();
        assert_eq!(loaded.nodes(), graph.nodes());
        assert_eq!(loaded.edges(), graph.edges());
        assert_eq!(loaded.outgoing(NodeId(0)).unwrap(), &[EdgeId(0)]);
        assert_eq!(loaded.turn_restrictions(), graph.turn_restrictions());
        assert!(!loaded.transition_allowed(EdgeId(0), EdgeId(1)).unwrap());
    }

    #[test]
    fn rejects_wrong_magic_and_version() {
        let graph = graph();
        let wrong_magic = encode(GraphEnvelope {
            magic: *b"NOTGRAPH",
            schema_version: GRAPH_SCHEMA_VERSION,
            graph: graph.clone(),
        });
        assert!(matches!(
            decode_graph(&wrong_magic),
            Err(Error::InvalidMagic { .. })
        ));

        let wrong_version = encode(GraphEnvelope {
            magic: GRAPH_MAGIC,
            schema_version: GRAPH_SCHEMA_VERSION + 1,
            graph,
        });
        assert!(matches!(
            decode_graph(&wrong_version),
            Err(Error::UnsupportedSchemaVersion { .. })
        ));
    }

    #[test]
    fn rejects_truncated_and_trailing_data() {
        let bytes = encode(GraphEnvelope {
            magic: GRAPH_MAGIC,
            schema_version: GRAPH_SCHEMA_VERSION,
            graph: graph(),
        });
        assert!(matches!(
            decode_graph(&bytes[..bytes.len() - 1]),
            Err(Error::Decode(_))
        ));
        let mut trailing = bytes;
        trailing.push(0);
        assert!(matches!(
            decode_graph(&trailing),
            Err(Error::TrailingData(1))
        ));
    }

    #[test]
    fn rejects_oversized_file_from_metadata_without_reading_it() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("oversized.myroute");
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(MAX_GRAPH_FILE_SIZE_BYTES + 1).unwrap();
        assert!(matches!(
            load_graph(&path),
            Err(Error::GraphTooLarge {
                size,
                max: MAX_GRAPH_FILE_SIZE_BYTES
            }) if size == MAX_GRAPH_FILE_SIZE_BYTES + 1
        ));
    }

    #[test]
    fn decode_limit_rejects_payload_before_deserializing_graph() {
        let bytes = encode(GraphEnvelope {
            magic: GRAPH_MAGIC,
            schema_version: GRAPH_SCHEMA_VERSION,
            graph: graph(),
        });
        assert!(matches!(
            decode_graph_with_limit::<16>(&bytes),
            Err(Error::Decode(bincode::error::DecodeError::LimitExceeded))
        ));
    }

    fn encode(envelope: GraphEnvelope) -> Vec<u8> {
        bincode::serde::encode_to_vec(envelope, bincode::config::standard()).unwrap()
    }

    fn graph() -> RoadGraph {
        let nodes = vec![
            Node::new(NodeId(0), Coordinate::new(40.0, -74.0).unwrap()),
            Node::new(NodeId(1), Coordinate::new(40.001, -74.0).unwrap()),
            Node::new(NodeId(2), Coordinate::new(40.002, -74.0).unwrap()),
        ];
        let edges = vec![
            Edge::new(EdgeId(0), NodeId(0), NodeId(1), 111.0, RoadClass::Primary),
            Edge::new(EdgeId(1), NodeId(1), NodeId(2), 111.0, RoadClass::Primary),
        ];
        RoadGraph::with_turn_restrictions(
            nodes,
            edges,
            vec![TurnRestriction::new(EdgeId(0), EdgeId(1))],
        )
        .unwrap()
    }
}
