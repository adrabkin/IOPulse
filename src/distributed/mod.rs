//! Distributed mode implementation
//!
//! This module implements distributed testing across multiple nodes.
//!
//! # Architecture
//!
//! IOPulse distributed mode uses a coordinator-node architecture:
//!
//! - **Coordinator**: Orchestrates the test, connects to all nodes, aggregates results
//! - **Node Service**: Runs on nodes, spawns worker threads, reports statistics
//! - **Workers**: Threads on nodes executing IO operations
//!
//! # Modules
//!
//! - `protocol`: Message definitions and serialization
//! - `node_service`: Node service implementation (Task 27)
//! - `coordinator`: Distributed coordinator implementation (Task 28)

pub mod protocol;
pub mod node_service;
pub mod coordinator;

// Re-export key types
pub use protocol::{
    Message,
    PrepareFilesMessage,
    FilesReadyMessage,
    ConfigMessage,
    ReadyMessage,
    StartMessage,
    HeartbeatMessage,
    ResultsMessage,
    ErrorMessage,
    WorkerStatsSnapshot,
    PROTOCOL_VERSION,
};

pub use node_service::NodeService;
pub use coordinator::DistributedCoordinator;
