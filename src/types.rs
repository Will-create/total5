// Total-rs framework types
// The MIT License
// Copyright 2012-2023 (c) Louis Bertson <louisbertsonpetersirka@gmail.com>

use std::collections::HashMap;
use std::time::Duration;
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


pub struct Config {
    // Regular properties
    pub name: String,
    pub version: String,
    pub author: String,
    pub secret: String,
    pub secret_encryption: String,
    pub secret_totalapi: String,
    pub secret_csrf: String,
    pub secret_tapi: String,
    pub secret_tms: String,

    // Properties with $ prefix
    pub _root: String,
    pub _cors: String,
    pub _api: String,
    pub _sourcemap: bool,
    pub _httpreqlimit: usize,
    pub _httpcompress: bool,
    pub _httpetag: String,
    pub _httpmaxsize: usize,
    pub _httprangebuffer: usize,
    pub _httptimeout: u64,
    pub _httpfiles: HashMap<String, bool>,
    pub _httpchecktypes: bool,
    pub _httpmaxage: u64,
    pub _httpmaxkeys: usize,
    pub _httpmaxkey: usize,
    pub _blacklist: String,
    pub _xpoweredby: String,
    pub _maxopenfiles: usize,
    pub _minifyjs: bool,
    pub _minifycss: bool,
    pub _minifyhtml: bool,
    pub _localize: bool,
    pub _port: String,
    pub _ip: String,
    pub _unixsocket: String,
    pub _timezone: String,
    pub _insecure: bool,
    pub _performance: bool,
    pub _filtererrors: bool,
    pub _cleartemp: bool,
    pub _customtitles: bool,
    pub _version: String,
    pub _clearcache: usize,
    pub _imageconverter: String,
    pub _imagememory: usize,
    pub _stats: bool,
    pub _npmcache: String,
    pub _python: String,
    pub _wsmaxsize: usize,
    pub _wscompress: bool,
    pub _wsencodedecode: bool,
    pub _wsmaxlatency: usize,
    pub _proxytimeout: u64,
    pub _cookiesamesite: String,
    pub _cookiesecure: bool,
    pub _csrfexpiration: String,
    pub _tapi: bool,
    pub _tapiurl: String,
    pub _tapimail: bool,
    pub _tapilogger: bool,
    pub _imprint: bool,
    pub _tms: bool,
    pub _tmsmaxsize: usize,
    pub _tmsurl: String,
    pub _tmsclearblocked: usize,
    pub mail_from: Option<String>,
    pub mail_from_name: Option<String>,
    pub mail_reply: Option<String>,
    pub mail_cc: Option<String>,
    pub mail_bcc: Option<String>,
    pub smtp: SMTPConfig,
}

/// Framework statistics
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


pub struct DEF {
    pub helpers: HashMap<String, Box<dyn Fn() + Send + Sync>>,
    pub currencies: HashMap<String, Currency>,
    pub parsers: Parsers,
    pub validators: Validators,
}

pub struct Currency {
    // Currency properties
    pub code: String,
    pub symbol: String,
}

pub struct Parsers {
    pub json: Box<dyn Fn(&str) -> Result<serde_json::Value, serde_json::Error> + Send + Sync>,
    pub urlencoded: Box<dyn Fn(&str) -> HashMap<String, String> + Send + Sync>,
    pub xml: Box<dyn Fn(&str) -> Result<String, String> + Send + Sync>,
}

pub struct Validators {
    pub email: regex::Regex,
    pub url: regex::Regex,
    pub phone: regex::Regex,
    pub zip: regex::Regex,
    pub uid: regex::Regex,
    pub xss: regex::Regex,
    pub sqlinjection: regex::Regex,
}

pub struct Controller {
    pub ip: String,
    pub headers: HashMap<String, String>,
    pub query: HashMap<String, String>,
}

pub struct TMail {
    // Email structure
}

pub struct Message {
    pub subject: String,
    pub body: String,
    pub to_addresses: Vec<String>,
    pub from_address: Option<String>,
    pub from_name: Option<String>,
    pub reply_to: Option<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub _sending: Option<std::time::Instant>,
}
pub struct Path {
    pub root: String,
    pub logs: String,
    pub scripts: String,
    pub public: String,
    pub private: String,
    pub databases: String,
    pub plugins: String,
    pub templates: String,
    pub flowstreams: String,
    pub modules: String,
    pub tmp: String,
}
pub struct TPath {
    pub base_dir: String,
    pub temporary_dirs: HashMap<String, String>,
}


pub struct CronJob {
    // Cron job properties
    pub id: String,
    pub schedule: String,
    pub handler: Box<dyn Fn() + Send + Sync>,
}

pub struct ErrorInfo {
    pub error: String,
    pub name: Option<String>,
    pub url: Option<String>,
    pub date: DateTime<Utc>,
}


pub struct SMTPConfig {
    pub from: Option<String>,
    pub name: Option<String>,
    pub user: Option<String>,
}

pub struct Internal {
    pub ticks: u64,
    pub counter: u64,
    pub uid: u64,
    pub interval: Option<std::time::Duration>,
}

pub struct Route {
    // Route properties
    pub path: String,
    pub handler: Box<dyn Fn() + Send + Sync>,
}

pub struct WebSocketRoute {
    // WebSocket route properties
    pub path: String,
    pub handler: Box<dyn Fn() + Send + Sync>,
}

pub struct WebSocketConnection {
    // WebSocket connection properties
    pub id: String,
    pub socket: Box<dyn std::any::Any + Send + Sync>,
}

pub struct FileRoute {
    // File route properties
    pub path: String,
    pub handler: Box<dyn Fn() + Send + Sync>,
}

pub struct MiddlewareHandler {
    // Middleware handler properties
    pub handler: Box<dyn Fn() + Send + Sync>,
}

pub struct ImageMiddlewareHandler {
    // Image middleware handler properties
    pub handler: Box<dyn Fn() + Send + Sync>,
}

pub struct Proxy {
    // Proxy properties
    pub source: String,
    pub target: String,
}
pub struct CryptoKey {
    // Crypto key properties
    pub key: String,
}

pub struct DDOSEntry {
    // DDOS entry properties
    pub count: u32,
    pub expires: DateTime<Utc>,
}

pub struct Service {
    pub redirect: u64,
    pub request: u64,
    pub file: u64,
    pub usage: u64,
}

pub struct PendingItem {
    // Pending item properties
    pub id: String,
    pub created: DateTime<Utc>,
}

pub struct Ban {
    // Ban properties
    pub ip: String,
    pub expires: DateTime<Utc>,
}

pub struct DateTimeFormatter {
    // DateTime formatter properties
    pub format: String,
}


pub struct Performance {
    pub publish: u64,
    pub subscribe: u64,
    pub calls: u64,
    pub download: u64,
    pub upload: u64,
    pub request: u64,
    pub message: u64,
    pub file: u64,
    pub open: u64,
    pub online: u64,
    pub usage: u64,
    pub mail: u64,
    pub dbrm: u64,
    pub dbwm: u64,
    pub external: u64,
}


pub struct SuccessResult<T> {
    pub success: bool,
    pub value: T,
}

pub struct AuditData {
    pub dtcreated: DateTime<Utc>,
    // Other audit data fields would go here
}
