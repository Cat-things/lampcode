//! Local SQLite storage in the app data dir.

pub mod db;
pub mod settings;

pub use db::{Db, StorageError};
pub use settings::{Settings, Theme, UpdateCheck};
