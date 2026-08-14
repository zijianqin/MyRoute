use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed to open OSM PBF `{path}`: {source}")]
    OpenPbf {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to parse OSM PBF `{path}`: {source}")]
    ParsePbf {
        path: PathBuf,
        source: osmpbfreader::Error,
    },
    #[error("OSM way {way_id} references missing node {node_id}")]
    MissingNodeDependency { way_id: i64, node_id: i64 },
    #[error("import has too many {kind} for dense u32 IDs: {count}")]
    IdCapacity { kind: &'static str, count: usize },
    #[error("failed to read graph `{path}`: {source}")]
    ReadGraph {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to write graph `{path}`: {source}")]
    WriteGraph {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("graph is {size} bytes, exceeding the {max}-byte regional graph limit")]
    GraphTooLarge { size: u64, max: u64 },
    #[error("failed to encode graph: {0}")]
    Encode(#[from] bincode::error::EncodeError),
    #[error("failed to decode graph: {0}")]
    Decode(#[from] bincode::error::DecodeError),
    #[error("invalid graph magic: expected {expected:?}, found {actual:?}")]
    InvalidMagic { expected: [u8; 8], actual: [u8; 8] },
    #[error(
        "unsupported graph schema version {actual}; supported version is {supported}; re-import the original .osm.pbf with `myroute import`"
    )]
    UnsupportedSchemaVersion { actual: u16, supported: u16 },
    #[error("graph file contains {0} trailing bytes")]
    TrailingData(usize),
    #[error("invalid road graph: {0}")]
    InvalidGraph(#[from] myroute_core::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
