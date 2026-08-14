//! OpenStreetMap import and graph persistence for MyRoute.

mod builder;
mod error;
mod parser;
mod persistence;

pub use error::{Error, Result};
pub use parser::{ImportReport, import_pbf};
pub use persistence::{
    GRAPH_MAGIC, GRAPH_SCHEMA_VERSION, MAX_GRAPH_FILE_SIZE_BYTES, load_graph, save_graph,
};
