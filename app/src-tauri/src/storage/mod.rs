//! Local SQLite storage in the app data dir.

pub mod db;
pub mod sessions;
pub mod settings;

pub use db::{Db, StorageError};
pub use settings::{ModelRef, Settings, Theme, UpdateCheck};
