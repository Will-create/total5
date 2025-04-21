// Totaljs v5 framework implementation in Rust
// The MIT License
// Copyright 2012-2023 (c) Louis Bertson <louisbertsonpetersirka@gmail.com>
use once_cell::sync::Lazy;
use std::path::PathBuf;


mod types;
mod utils;
mod globals;

// Re-export the main components for library users
pub use types::{Framework, FrameworkValue, ClusterStats};
pub use utils::TPath;
pub use globals::{EMPTY_ARRAY, EMPTY_OBJECT, REG_HTTPHTTPS, REG_SKIPERRORS, SOCKETWINDOWS, IGNORE_AUDIT};


/// Initialize a new framework instance with default values
pub fn initialize_framework() -> Framework {
    Framework::default()
}

/// Create a new PathUtils instance with the given base directory
pub static VERSION: &str = "5.0.0";
pub static F: Lazy<Framework> = Lazy::new(|| Framework::default());
pub static PATH: Lazy<TPath> = Lazy::new(|| TPath::new(PathBuf::from("src")));