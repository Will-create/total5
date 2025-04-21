// Total-rs framework types
// The MIT License
// Copyright 2012-2023 (c) Louis Bertson <louisbertsonpetersirka@gmail.com>

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use chrono::{DateTime, Utc};

/// Framework value types that can be stored in various collections
#[derive(Debug, Clone)]
pub enum FrameworkValue {
    String(String),
    Number(i64),
    Float(f64),
    Boolean(bool),
    Object(HashMap<String, FrameworkValue>),
    Array(Vec<FrameworkValue>),
    Null,
}

impl Default for FrameworkValue {
    fn default() -> Self {
        FrameworkValue::Null
    }
}

impl From<&str> for FrameworkValue {
    fn from(s: &str) -> Self {
        FrameworkValue::String(s.to_string())
    }
}

impl From<String> for FrameworkValue {
    fn from(s: String) -> Self {
        FrameworkValue::String(s)
    }
}

impl From<i64> for FrameworkValue {
    fn from(n: i64) -> Self {
        FrameworkValue::Number(n)
    }
}

impl From<f64> for FrameworkValue {
    fn from(f: f64) -> Self {
        FrameworkValue::Float(f)
    }
}

impl From<bool> for FrameworkValue {
    fn from(b: bool) -> Self {
        FrameworkValue::Boolean(b)
    }
}

impl From<HashMap<String, FrameworkValue>> for FrameworkValue {
    fn from(map: HashMap<String, FrameworkValue>) -> Self {
        FrameworkValue::Object(map)
    }
}

impl From<Vec<FrameworkValue>> for FrameworkValue {
    fn from(vec: Vec<FrameworkValue>) -> Self {
        FrameworkValue::Array(vec)
    }
}

/// Stats about cluster operations
#[derive(Debug, Clone)]
pub struct ClusterStats {
    pub r#type: String,
}

impl Default for ClusterStats {
    fn default() -> Self {
        Self {
            r#type: "stats".to_string(),
        }
    }
}

/// Internal framework statistics
#[derive(Debug, Clone, Default)]
pub struct InternalStats {
    pub ticks: i64,
    pub counter: i64,
    pub uid: i64,
    pub interval: Option<i64>,
}

/// Performance statistics
#[derive(Debug, Clone, Default)]
pub struct PerformanceStats {
    pub publish: i64,
    pub subscribe: i64,
    pub calls: i64,
    pub download: i64,
    pub upload: i64,
    pub request: i64,
    pub message: i64,
    pub file: i64,
    pub open: i64,
    pub online: i64,
    pub usage: i64,
    pub mail: i64,
    pub dbrm: i64,
    pub dbwm: i64,
    pub external: i64,
}

/// Other statistics
#[derive(Debug, Clone, Default)]
pub struct OtherStats {
    pub websocketping: i64,
    pub websocketcleaner: i64,
    pub obsolete: i64,
    pub mail: i64,
}

/// Request statistics
#[derive(Debug, Clone, Default)]
pub struct RequestStats {
    pub request: i64,
    pub external: i64,
    pub pending: i64,
    pub web: i64,
    pub xhr: i64,
    pub file: i64,
    pub websocket: i64,
    pub get: i64,
    pub options: i64,
    pub head: i64,
    pub post: i64,
    pub put: i64,
    pub patch: i64,
    pub upload: i64,
    pub schema: i64,
    pub operation: i64,
    pub blocked: i64,
    pub delete: i64,
    pub mobile: i64,
    pub desktop: i64,
    pub size: i64,
}

/// Response statistics
#[derive(Debug, Clone, Default)]
pub struct ResponseStats {
    pub ddos: i64,
    pub html: i64,
    pub xml: i64,
    pub json: i64,
    pub websocket: i64,
    pub timeout: i64,
    pub custom: i64,
    pub binary: i64,
    pub pipe: i64,
    pub file: i64,
    pub image: i64,
    pub destroy: i64,
    pub stream: i64,
    pub streaming: i64,
    pub text: i64,
    pub empty: i64,
    pub redirect: i64,
    pub forward: i64,
    pub proxy: i64,
    pub notmodified: i64,
    pub sse: i64,
    pub errorbuilder: i64,
    pub error400: i64,
    pub error401: i64,
    pub error403: i64,
    pub error404: i64,
    pub error409: i64,
    pub error431: i64,
    pub error500: i64,
    pub error501: i64,
    pub error503: i64,
    pub size: i64,
}

/// Framework statistics
#[derive(Debug, Clone, Default)]
pub struct Stats {
    pub compilation: i64,
    pub error: i64,
    pub performance: PerformanceStats,
    pub other: OtherStats,
    pub request: RequestStats,
    pub response: ResponseStats,
}

/// Service statistics
#[derive(Debug, Clone, Default)]
pub struct ServiceStats {
    pub redirect: i64,
    pub request: i64,
    pub file: i64,
    pub usage: i64,
}

/// Temporary storage
#[derive(Debug, Clone, Default)]
pub struct Temporary {
    pub path: HashMap<String, FrameworkValue>,
    pub actions: HashMap<String, FrameworkValue>,
    pub cache: HashMap<String, FrameworkValue>,
    pub notfound: HashMap<String, FrameworkValue>,
    pub processing: HashMap<String, FrameworkValue>,
    pub views: HashMap<String, FrameworkValue>,
    pub viewscache: Vec<FrameworkValue>,
    pub directories: HashMap<String, FrameworkValue>,
    pub versions: HashMap<String, FrameworkValue>,
    pub dependencies: HashMap<String, FrameworkValue>,
    pub other: HashMap<String, FrameworkValue>,
    pub cryptokeys: HashMap<String, FrameworkValue>,
    pub internal: HashMap<String, FrameworkValue>,
    pub ready: HashMap<String, FrameworkValue>,
    pub ddos: HashMap<String, FrameworkValue>,
    pub service: ServiceStats,
    pub pending: Vec<FrameworkValue>,
    pub tmp: HashMap<String, FrameworkValue>,
    pub merged: HashMap<String, FrameworkValue>,
    pub minified: HashMap<String, FrameworkValue>,
    pub tmsblocked: HashMap<String, FrameworkValue>,
    pub dnscache: HashMap<String, FrameworkValue>,
    pub blocked: HashMap<String, FrameworkValue>,
    pub bans: HashMap<String, FrameworkValue>,
    pub calls: HashMap<String, FrameworkValue>,
    pub utils: HashMap<String, FrameworkValue>,
    pub mail: HashMap<String, FrameworkValue>,
    pub images: HashMap<String, FrameworkValue>,
    pub querybuilders: HashMap<String, FrameworkValue>,
    pub templates: HashMap<String, FrameworkValue>,
    pub smtp: HashMap<String, FrameworkValue>,
    pub datetime: HashMap<String, FrameworkValue>,
}

/// Routes configuration
#[derive(Debug, Clone, Default)]
pub struct Routes {
    pub fallback: HashMap<String, FrameworkValue>,
    pub virtual_routes: HashMap<String, FrameworkValue>,
    pub api: HashMap<String, FrameworkValue>,
    pub routes: Vec<FrameworkValue>,
    pub routescache: HashMap<String, FrameworkValue>,
    pub websockets: Vec<FrameworkValue>,
    pub websocketscache: HashMap<String, FrameworkValue>,
    pub files: Vec<FrameworkValue>,
    pub filescache: HashMap<String, FrameworkValue>,
    pub timeout: Option<i64>,
    pub middleware: HashMap<String, FrameworkValue>,
    pub imagesmiddleware: HashMap<String, FrameworkValue>,
    pub proxies: Vec<FrameworkValue>,
}

/// Main Framework structure
#[derive(Debug, Clone)]
pub struct Framework {
    pub id: String,
    pub clusterid: String,
    pub is5: i64,
    pub version: i64,
    pub is_bundle: bool,
    pub is_loaded: bool,
    pub version_header: String,
    pub version_node: String,
    
    // Collections
    pub resources: HashMap<String, FrameworkValue>,
    pub connections: HashMap<String, FrameworkValue>,
    pub schedules: HashMap<String, FrameworkValue>,
    pub modules: HashMap<String, FrameworkValue>,
    pub plugins: HashMap<String, FrameworkValue>,
    pub actions: HashMap<String, FrameworkValue>,
    pub apiservices: HashMap<String, FrameworkValue>,
    pub processing: HashMap<String, FrameworkValue>,
    pub transformations: HashMap<String, FrameworkValue>,
    pub consumption: HashMap<String, FrameworkValue>,
    pub flowstreams: HashMap<String, FrameworkValue>,
    pub filestorages: HashMap<String, FrameworkValue>,
    pub jsonschemas: HashMap<String, FrameworkValue>,
    pub querybuilders: HashMap<String, FrameworkValue>,
    pub openclients: HashMap<String, FrameworkValue>,
    pub nodemodules: HashMap<String, FrameworkValue>,
    pub workers: HashMap<String, FrameworkValue>,
    
    // Arrays
    pub timeouts: Vec<FrameworkValue>,
    pub errors: Vec<FrameworkValue>,
    pub paused: Vec<FrameworkValue>,
    pub crons: Vec<FrameworkValue>,
    
    // Complex objects
    pub internal: InternalStats,
    pub routes: Routes,
    pub temporary: Temporary,
    pub stats: Stats,
}

impl Default for Framework {
    fn default() -> Self {
        Self {
            id: String::new(),
            clusterid: String::new(),
            is5: 5012,
            version: 5012,
            is_bundle: false,
            is_loaded: false,
            version_header: "5".to_string(),
            version_node: std::env::var("NODE_VERSION").unwrap_or_else(|_| "unknown".to_string()),
            
            resources: HashMap::new(),
            connections: HashMap::new(),
            schedules: HashMap::new(),
            modules: HashMap::new(),
            plugins: HashMap::new(),
            actions: HashMap::new(),
            apiservices: HashMap::new(),
            processing: HashMap::new(),
            transformations: HashMap::new(),
            consumption: HashMap::new(),
            flowstreams: HashMap::new(),
            filestorages: HashMap::new(),
            jsonschemas: HashMap::new(),
            querybuilders: HashMap::new(),
            openclients: HashMap::new(),
            nodemodules: HashMap::new(),
            workers: HashMap::new(),
            
            timeouts: Vec::new(),
            errors: Vec::new(),
            paused: Vec::new(),
            crons: Vec::new(),
            
            internal: InternalStats::default(),
            routes: Routes::default(),
            temporary: Temporary::default(),
            stats: Stats::default(),
        }
    }
}