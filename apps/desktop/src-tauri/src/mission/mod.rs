//! Missions (§29–§35).

pub mod model;
pub mod store;
pub mod verify;

pub use model::{
    Autonomy, AcceptanceCriterion, Evidence, Mission, MissionDetail, MissionStatus, MissionTask,
    Verification,
};

use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum MissionError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Db(#[from] crate::db::DbError),
    #[error("no such mission: {0}")]
    Unknown(String),
    #[error("a mission needs a title")]
    MissingTitle,
    #[error(
        "cannot complete: {count} required acceptance criteria are not verified. \
         A mission is complete when there is evidence, not when it is claimed."
    )]
    NotVerified { count: usize },
}

impl Serialize for MissionError {
    // Fully qualified: the crate-local `Result<T>` alias below would otherwise
    // shadow the two-parameter std type this signature needs.
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

pub type Result<T> = std::result::Result<T, MissionError>;
