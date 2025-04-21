// Total-rs framework globals
// The MIT License
// Copyright 2012-2023 (c) Louis Bertson <louisbertsonpetersirka@gmail.com>

use std::collections::HashMap;
use std::sync::Arc;
use once_cell::sync::Lazy;
use regex::Regex;

// Empty collections - using Arc to make them shareable and immutable
pub static EMPTY_OBJECT: Lazy<Arc<HashMap<(), ()>>> = Lazy::new(|| Arc::new(HashMap::new()));
pub static EMPTY_ARRAY: Lazy<Arc<Vec<()>>> = Lazy::new(|| Arc::new(Vec::new()));

// Regular expressions
pub static REG_SKIPERRORS: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"epipe|invalid\sdistance|err_ipc_channel_closed").unwrap()
});

pub static REG_HTTPHTTPS: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^(http|https)://").unwrap()
});

pub const SOCKETWINDOWS: &str = r"\\?\pipe";

// HashMap for IGNORE_AUDIT
pub static IGNORE_AUDIT: Lazy<HashMap<&'static str, i32>> = Lazy::new(|| {
    let mut map = HashMap::new();
    map.insert("password", 1);
    map.insert("token", 1);
    map.insert("accesstoken", 1);
    map.insert("access_token", 1);
    map.insert("pin", 1);
    map
});

// Static mutable variable for CONCAT - this would better be handled through a mutex or other thread-safe approach
pub static mut CONCAT: [Option<String>; 2] = [None, None];