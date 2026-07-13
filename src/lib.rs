//! Total.rs HTTP/API core.
//!
//! The first milestone focuses on the Total.js feeling for backend APIs:
//! convention paths, compact route registration, JSON responses, actions, and
//! safe static files.

pub mod server;
mod statistics;
mod storage;
mod utils;

pub use statistics::StatisticsSnapshot;
use statistics::{
    ip_is_blacklisted, request_ip, request_limit_exceeded, PendingRequest, RequestStatistics,
};
pub use storage::{Data, Db, FileStorage, PgDb, QueryBuilder, StoredFile};

use base64::Engine;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use http::{header, HeaderMap, Method, StatusCode, Uri};
use once_cell::sync::Lazy;
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::fs as stdfs;
use std::future::Future;
use std::io::Write;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, RwLock};
use tokio::fs;

pub use utils::TPath;

type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;
type Handler = Arc<dyn Fn(Context) -> BoxFuture<Response> + Send + Sync>;
type Action = Arc<dyn Fn(Context, Value) -> BoxFuture<Result<Value, Error>> + Send + Sync>;
type Middleware = Arc<dyn Fn(Context) -> BoxFuture<Result<Context, Response>> + Send + Sync>;
type CustomValidator = Arc<dyn Fn(&Value) -> bool + Send + Sync>;
type LifecycleHook = Arc<dyn Fn(&mut Total) -> Result<(), Error> + Send + Sync>;
type FlowHandler = Arc<dyn Fn(FlowMessage) -> BoxFuture<Result<Value, Error>> + Send + Sync>;
type WsHandler = Arc<dyn Fn(WsContext) -> BoxFuture<()> + Send + Sync>;
type AuthHandler = Arc<dyn Fn(Context) -> BoxFuture<Result<Context, Response>> + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum RouteAuth {
    #[default]
    Any,
    Member,
    Guest,
}

/// Framework version exposed to applications.
pub const VERSION: &str = "0.1.0";

/// Runtime configuration with Total.js-like defaults.
#[derive(Debug, Clone)]
pub struct Config {
    pub name: String,
    pub version: String,
    pub ip: String,
    pub port: u16,
    pub body_limit: usize,
    pub handler_timeout: u64,
    pub public_max_age: u64,
    pub x_powered_by: String,
    pub values: HashMap<String, Value>,
    binary_values: HashMap<String, Vec<u8>>,
}

impl Default for Config {
    fn default() -> Self {
        let mut values = total_config_defaults();
        values.insert("name".to_string(), json!("Total.rs"));
        values.insert("version".to_string(), json!("0.1.0"));
        Self {
            name: "Total.rs".to_string(),
            version: "0.1.0".to_string(),
            ip: "0.0.0.0".to_string(),
            port: 8000,
            body_limit: 256 * 1024,
            handler_timeout: 5,
            public_max_age: 60,
            x_powered_by: "Total.rs".to_string(),
            values,
            binary_values: HashMap::new(),
        }
    }
}

fn total_config_defaults() -> HashMap<String, Value> {
    let files = "flac,jpg,jpeg,png,gif,ico,wasm,js,mjs,css,txt,xml,woff,woff2,otf,ttf,eot,svg,zip,rar,pdf,docx,xlsx,doc,xls,html,htm,appcache,manifest,map,ogv,ogg,mp4,mp3,webp,webm,swf,package,json,ui,md,m4v,jsx,heif,heic,ics,ts,m3u8,wav,xsd,xsl,xslt,ipynb,ijsnb,log,webmanifest";
    let mut values = HashMap::new();
    for (key, value) in [
        ("author", json!("")),
        ("secret", json!("")),
        ("secret_encryption", json!("")),
        ("secret_totalapi", json!("")),
        ("secret_csrf", json!("")),
        ("secret_tapi", json!("")),
        ("secret_tms", json!("")),
        ("$root", json!("")),
        ("$cors", json!("")),
        ("$api", json!("/api/")),
        ("$sourcemap", json!(true)),
        ("$httpreqlimit", json!(0)),
        ("$httpcompress", json!(true)),
        ("$httpetag", json!("")),
        ("$httpmaxsize", json!(256)),
        ("$httprangebuffer", json!(5120)),
        ("$httptimeout", json!(5)),
        ("$httpchecktypes", json!(true)),
        ("$httpmaxage", json!(60)),
        ("$httpmaxkeys", json!(33)),
        ("$httpmaxkey", json!(25)),
        ("$httpcacheoffset", json!("")),
        ("$blacklist", json!("")),
        ("$xpoweredby", json!("Total.rs")),
        ("$maxopenfiles", json!(100)),
        ("$minifyjs", json!(true)),
        ("$minifycss", json!(true)),
        ("$minifyhtml", json!(true)),
        ("$localize", json!(true)),
        ("$port", json!("auto")),
        ("$ip", json!("0.0.0.0")),
        ("$unixsocket", json!("")),
        ("$timezone", json!("utc")),
        ("$insecure", json!(false)),
        ("$performance", json!(false)),
        ("$filtererrors", json!(true)),
        ("$cleartemp", json!(true)),
        ("$customtitles", json!(false)),
        ("$version", json!("")),
        ("$clearcache", json!(10)),
        ("$imageconverter", json!("gm")),
        ("$imagememory", json!(0)),
        ("$stats", json!(true)),
        ("$npmcache", json!("/var/www/.npm")),
        ("$python", json!("python3")),
        ("$wsmaxsize", json!(256)),
        ("$wscompress", json!(true)),
        ("$wsencodedecode", json!(false)),
        ("$wsmaxlatency", json!(2000)),
        ("$proxytimeout", json!(5)),
        ("$cookiesamesite", json!("Lax")),
        ("$cookiesecure", json!(false)),
        ("$csrfexpiration", json!("30 minutes")),
        ("$tapi", json!(true)),
        ("$tapiurl", json!("eu")),
        ("$tapimail", json!(false)),
        ("$tapilogger", json!(false)),
        ("$imprint", json!(true)),
        ("$tms", json!(false)),
        ("$tmsmaxsize", json!(256)),
        ("$tmsurl", json!("/$tms/")),
        ("$tmsclearblocked", json!(60)),
    ] {
        values.insert(key.to_string(), value);
    }
    values.insert(
        "$httpfiles".to_string(),
        Value::Object(
            files
                .split(',')
                .map(|ext| (ext.to_string(), Value::Bool(true)))
                .collect(),
        ),
    );
    values
}

/// Global Total.js-style configuration snapshot.
///
/// Prefer `app.config()` or `ctx.conf()` when an application instance is
/// available. This static mirrors the latest parsed/runtime config for code
/// that needs the Total.js-like global `CONF` shape.
pub static CONF: Lazy<RwLock<Config>> = Lazy::new(|| RwLock::new(Config::default()));

pub fn conf() -> Config {
    CONF.read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

fn sync_global_conf(config: &Config) {
    *CONF
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = config.clone();
}

impl Config {
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.values.get(name)
    }

    pub fn string(&self, name: &str) -> Option<&str> {
        self.values.get(name).and_then(Value::as_str)
    }

    pub fn boolean(&self, name: &str) -> Option<bool> {
        self.values.get(name).and_then(Value::as_bool)
    }

    pub fn integer(&self, name: &str) -> Option<i64> {
        self.values.get(name).and_then(Value::as_i64)
    }

    pub fn number(&self, name: &str) -> Option<f64> {
        self.values.get(name).and_then(Value::as_f64)
    }

    pub fn array(&self, name: &str) -> Option<&Vec<Value>> {
        self.values.get(name).and_then(Value::as_array)
    }

    pub fn object(&self, name: &str) -> Option<&Map<String, Value>> {
        self.values.get(name).and_then(Value::as_object)
    }

    pub fn bytes(&self, name: &str) -> Option<&[u8]> {
        self.binary_values.get(name).map(Vec::as_slice)
    }

    pub fn set(&mut self, name: impl Into<String>, value: impl Serialize) -> Result<(), Error> {
        let mut name = name.into();
        let mut value = serde_json::to_value(value)
            .map_err(|err| Error::internal(format!("failed to serialize config value: {err}")))?;
        self.apply_special_entry(&mut name, &mut value);
        self.apply_known_value(&name, &value);
        self.values.insert(name, value);
        self.finalize_values();
        sync_global_conf(self);
        Ok(())
    }

    pub fn load_total_config(&mut self, path: impl AsRef<Path>) -> Result<(), Error> {
        let path = path.as_ref();
        let cache_path = path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("databases/config.json");
        self.load_total_config_with_cache(path, &cache_path)
    }

    fn load_total_config_with_cache(
        &mut self,
        path: &Path,
        cache_path: &Path,
    ) -> Result<(), Error> {
        let body = stdfs::read_to_string(path)
            .map_err(|err| Error::internal(format!("failed to read config: {err}")))?;
        let mut generated_cache: Map<String, Value> = stdfs::read_to_string(cache_path)
            .ok()
            .and_then(|body| serde_json::from_str(&body).ok())
            .unwrap_or_default();
        let mut cache_changed = false;
        for line in body.lines() {
            let line = strip_config_comment(line);
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
                continue;
            }
            let Some((name, raw)) = line.split_once(':') else {
                continue;
            };
            let (mut name, type_hint) = parse_config_name(name.trim());
            if name.is_empty() {
                continue;
            }
            let (mut value, persistent) =
                parse_config_value_with_type(raw.trim(), type_hint.as_deref(), &self.values)?;
            if persistent {
                if let Some(cached) = generated_cache.get(&name) {
                    value = cached.clone();
                } else {
                    generated_cache.insert(name.clone(), value.clone());
                    cache_changed = true;
                }
            }
            self.apply_special_entry(&mut name, &mut value);
            self.apply_known_value(&name, &value);
            self.values.insert(name.to_string(), value);
        }
        self.finalize_values();
        if cache_changed {
            if let Some(parent) = cache_path.parent() {
                stdfs::create_dir_all(parent).map_err(|err| {
                    Error::internal(format!(
                        "failed to create generated config directory: {err}"
                    ))
                })?;
            }
            stdfs::write(
                cache_path,
                serde_json::to_vec_pretty(&generated_cache).unwrap(),
            )
            .map_err(|err| Error::internal(format!("failed to persist generated config: {err}")))?;
        }
        sync_global_conf(self);
        Ok(())
    }

    pub fn load_env_file(&mut self, path: impl AsRef<Path>) -> Result<(), Error> {
        let body = stdfs::read_to_string(path)
            .map_err(|err| Error::internal(format!("failed to read .env: {err}")))?;
        for line in body.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
                continue;
            }
            let line = line.strip_prefix("export ").unwrap_or(line);
            let Some((key, raw)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            if key.is_empty() || std::env::var_os(key).is_some() {
                continue;
            }
            let value = unquote_config_value(raw.trim())
                .unwrap_or_else(|| raw.trim().to_string())
                .replace("\\n", "\n");
            std::env::set_var(key, value);
        }
        self.load_env();
        Ok(())
    }

    pub fn load_env(&mut self) {
        if let Ok(value) = std::env::var("PORT") {
            if let Ok(port) = value.parse::<u16>() {
                self.port = port;
                self.values.insert("port".to_string(), json!(port));
            }
        }
        if let Ok(value) = std::env::var("IP") {
            self.ip = value.clone();
            self.values.insert("ip".to_string(), json!(value));
        }
        if let Ok(value) = std::env::var("APP_NAME") {
            self.name = value.clone();
            self.values.insert("name".to_string(), json!(value));
        }
        sync_global_conf(self);
    }

    fn apply_known_value(&mut self, name: &str, value: &Value) {
        match name {
            "name" | "app_name" => {
                if let Some(value) = value.as_str() {
                    self.name = value.to_string();
                }
            }
            "version" => {
                if let Some(value) = value.as_str() {
                    self.version = value.to_string();
                }
            }
            "ip" | "$ip" => {
                if let Some(value) = value.as_str() {
                    self.ip = value.to_string();
                }
            }
            "port" | "$port" => {
                if let Some(value) = value.as_u64().and_then(|value| u16::try_from(value).ok()) {
                    self.port = value;
                }
            }
            "body_limit" | "body-limit" | "bodylimit" => {
                if let Some(value) = value.as_u64().and_then(|value| usize::try_from(value).ok()) {
                    self.body_limit = value;
                }
            }
            "$httpmaxsize" => {
                if let Some(value) = value.as_u64().and_then(|value| usize::try_from(value).ok()) {
                    self.body_limit = value.saturating_mul(1024);
                }
            }
            "handler_timeout" | "handler-timeout" | "httptimeout" | "http_timeout"
            | "$httptimeout" => {
                if let Some(value) = value.as_u64() {
                    self.handler_timeout = value;
                }
            }
            "public_max_age" | "public-max-age" | "publicmaxage" | "$httpmaxage" => {
                if let Some(value) = value.as_u64() {
                    self.public_max_age = value;
                }
            }
            "x_powered_by" | "x-powered-by" | "$xpoweredby" => {
                if let Some(value) = value.as_str() {
                    self.x_powered_by = value.to_string();
                }
            }
            _ => {}
        }
    }

    fn apply_special_entry(&mut self, name: &mut String, value: &mut Value) {
        if name == "totalapi" {
            *name = if value.as_str().is_some_and(|value| value.len() > 5) {
                "secret_totalapi".to_string()
            } else {
                "$tapi".to_string()
            };
        }
        if name == "$root" {
            if let Some(root) = value.as_str() {
                let normalized = format!("/{}", root.trim_matches('/'));
                *value = json!(if normalized == "/" { "" } else { &normalized });
            }
        } else if name == "$httpfiles" {
            if let Some(files) = value.as_str() {
                let mut enabled = self
                    .values
                    .get("$httpfiles")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                for extension in files
                    .split(',')
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    enabled.insert(extension.to_string(), Value::Bool(true));
                }
                *value = Value::Object(enabled);
            }
        } else if name == "$timezone" {
            if let Some(timezone) = value.as_str() {
                std::env::set_var("TZ", timezone);
            }
        } else if name == "$cryptoiv" {
            if let Some(encoded) = value.as_str() {
                let bytes = if encoded
                    .chars()
                    .any(|ch| ch.is_ascii_uppercase() || matches!(ch, '=' | '/' | '+'))
                {
                    base64::engine::general_purpose::STANDARD
                        .decode(encoded)
                        .ok()
                } else {
                    decode_hex_bytes(encoded).ok()
                };
                if let Some(bytes) = bytes {
                    self.binary_values.insert(name.clone(), bytes);
                }
            }
        }
    }

    fn finalize_values(&mut self) {
        if let Some(root) = self.string("$root").filter(|root| !root.is_empty()) {
            let api = self.string("$api").unwrap_or("/api/");
            if !api.starts_with(root) {
                self.values.insert(
                    "$api".to_string(),
                    json!(format!("{root}/{}", api.trim_start_matches('/'))),
                );
            }
        }
        if self.string("secret_uid").is_none() {
            self.values
                .insert("secret_uid".to_string(), json!(self.name.clone()));
        }
        if self.string("$httpetag").unwrap_or_default().is_empty() {
            self.values.insert(
                "$httpetag".to_string(),
                json!(self.version.replace(['.', ' '], "")),
            );
        }
        let smtp_source = self
            .values
            .get("smtp")
            .or_else(|| self.values.get("mail"))
            .cloned();
        let mut smtp = match smtp_source {
            Some(Value::Object(value)) => value,
            Some(Value::String(value)) => serde_json::from_str::<Map<String, Value>>(&value)
                .unwrap_or_else(|_| Map::from_iter([("server".to_string(), json!(value))])),
            _ => Map::new(),
        };
        if let Some(server) = self.values.get("mail_smtp").cloned() {
            smtp.insert("server".to_string(), server);
        }
        if let Some(options) = self
            .values
            .get("mail_smtp_options")
            .and_then(Value::as_object)
        {
            smtp.extend(options.clone());
        }
        if !smtp.contains_key("server") {
            if let Some(server) = ["smtp", "host", "hostname", "url"]
                .iter()
                .find_map(|key| smtp.get(*key).cloned())
            {
                smtp.insert("server".to_string(), server);
            }
        }
        if !smtp.is_empty() {
            self.values.insert("smtp".to_string(), Value::Object(smtp));
        }
    }
}

/// HTTP methods accepted by the framework router.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

impl RouteMethod {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_uppercase().as_str() {
            "GET" => Some(Self::Get),
            "POST" => Some(Self::Post),
            "PUT" => Some(Self::Put),
            "PATCH" => Some(Self::Patch),
            "DELETE" => Some(Self::Delete),
            _ => None,
        }
    }

    fn into_http_method(self) -> Method {
        match self {
            Self::Get => Method::GET,
            Self::Post => Method::POST,
            Self::Put => Method::PUT,
            Self::Patch => Method::PATCH,
            Self::Delete => Method::DELETE,
        }
    }
}

#[derive(Clone)]
struct RouteDef {
    method: RouteMethod,
    path: String,
    handler: Handler,
    auth: RouteAuth,
}

#[derive(Clone)]
struct ApiEndpointDef {
    action: String,
    params: Vec<(usize, String)>,
    auth: RouteAuth,
}

#[derive(Clone)]
struct WebSocketDef {
    path: String,
    handler: WsHandler,
    auth: RouteAuth,
    protocols: Vec<String>,
}

#[derive(Clone)]
struct AppState {
    paths: TPath,
    config: Config,
    actions: Arc<HashMap<String, Action>>,
    middlewares: Arc<Vec<Middleware>>,
    auth: Option<AuthHandler>,
    plugins: Arc<HashMap<String, Plugin>>,
    flowstreams: Arc<HashMap<String, FlowStream>>,
    data: Data,
    filestorage: FileStorage,
    stats: Arc<RequestStatistics>,
    ws_connections: Arc<Mutex<HashMap<String, tokio::sync::mpsc::Sender<Vec<u8>>>>>,
}

#[derive(Clone)]
pub struct NativeDispatcher {
    state: Arc<AppState>,
    routes: Arc<Vec<RouteDef>>,
    api_routes: Arc<HashMap<String, HashMap<String, ApiEndpointDef>>>,
    websockets: Arc<Vec<WebSocketDef>>,
}

/// Installable controller/plugin/module contract.
///
/// This is the Rust equivalent of a Total.js `exports.install = function() { ... }`
/// block: a module receives the app and registers its routes, actions, and
/// middleware.
pub trait Install {
    fn install(&self, app: &mut Total) -> Result<(), Error>;
}

impl<F> Install for F
where
    F: Fn(&mut Total) -> Result<(), Error>,
{
    fn install(&self, app: &mut Total) -> Result<(), Error> {
        self(app)
    }
}

/// Main application builder.
pub struct Total {
    root: PathBuf,
    paths: TPath,
    config: Config,
    routes: Vec<RouteDef>,
    api_routes: HashMap<String, HashMap<String, ApiEndpointDef>>,
    websockets: Vec<WebSocketDef>,
    actions: HashMap<String, Action>,
    middlewares: Vec<Middleware>,
    auth: Option<AuthHandler>,
    plugins: HashMap<String, Plugin>,
    flowstreams: HashMap<String, FlowStream>,
    data: Data,
    filestorage: FileStorage,
    stats: Arc<RequestStatistics>,
    lifecycle: Lifecycle,
}

impl Default for Total {
    fn default() -> Self {
        Self::new()
    }
}

impl Total {
    pub fn new() -> Self {
        let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let paths = TPath::new(root.clone());
        let data = Data::new(paths.databases(None));
        let filestorage = FileStorage::new(paths.private(Some("filestorage")));
        Self {
            root,
            paths,
            config: Config::default(),
            routes: Vec::new(),
            api_routes: HashMap::new(),
            websockets: Vec::new(),
            actions: HashMap::new(),
            middlewares: Vec::new(),
            auth: None,
            plugins: HashMap::new(),
            flowstreams: HashMap::new(),
            data,
            filestorage,
            stats: Arc::new(RequestStatistics::default()),
            lifecycle: Lifecycle::default(),
        }
    }

    /// Creates an application from the current project conventions.
    ///
    /// This loads `config` automatically when it exists and ensures
    /// well-known Total directories exist.
    pub fn convention() -> Result<Self, Error> {
        let mut app = Self::new();
        app.load_config()?;
        app.prepare_directories()?;
        Ok(app)
    }

    pub fn convention_dev() -> Result<Self, Error> {
        let mut app = Self::new();
        app.config.set("debug", true)?;
        app.config.set("mode", "development")?;
        app.load_config()?;
        app.prepare_directories()?;
        Ok(app)
    }

    pub fn root(mut self, root: impl Into<PathBuf>) -> Self {
        self.root = root.into();
        self.paths = TPath::new(self.root.clone());
        self.data = Data::new(self.paths.databases(None));
        self.filestorage = FileStorage::new(self.paths.private(Some("filestorage")));
        self
    }

    pub fn config_mut(&mut self) -> &mut Config {
        &mut self.config
    }

    pub fn statistics(&self) -> StatisticsSnapshot {
        self.stats.snapshot()
    }

    pub fn paths(&self) -> &TPath {
        &self.paths
    }

    pub fn load_config(&mut self) -> Result<&mut Self, Error> {
        let env_path = self.paths.root(Some(".env"));
        if env_path.is_file() {
            self.config.load_env_file(env_path)?;
        } else {
            self.config.load_env();
        }

        for name in ["config"] {
            let path = self.paths.root(Some(name));
            if path.is_file() {
                let cache = self.paths.databases(Some("config.json"));
                self.config.load_total_config_with_cache(&path, &cache)?;
            }
        }
        self.apply_configured_paths();
        let debug = self
            .config
            .get("debug")
            .and_then(Value::as_bool)
            .unwrap_or_else(|| {
                std::env::var("TOTAL_MODE")
                    .is_ok_and(|mode| mode == "debug" || mode == "development")
            });
        let mode_file = if debug {
            "config-debug"
        } else {
            "config-release"
        };
        let path = self.paths.root(Some(mode_file));
        if path.is_file() {
            let cache = self.paths.databases(Some("config.json"));
            self.config.load_total_config_with_cache(&path, &cache)?;
        }
        self.apply_configured_paths();

        let plugins = self.paths.plugins(None);
        if let Ok(entries) = stdfs::read_dir(plugins) {
            let mut configs = entries
                .filter_map(Result::ok)
                .map(|entry| entry.path().join("config"))
                .filter(|path| path.is_file())
                .collect::<Vec<_>>();
            configs.sort();
            for path in configs {
                let cache = self.paths.databases(Some("config.json"));
                self.config.load_total_config_with_cache(&path, &cache)?;
            }
        }
        let version = self.paths.root(Some("version"));
        if let Ok(body) = stdfs::read_to_string(version) {
            if let Some(value) = body
                .lines()
                .next()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                self.config.version = value.to_string();
                self.config
                    .values
                    .insert("version".to_string(), json!(value));
            }
        }
        self.config.finalize_values();
        self.apply_configured_paths();
        sync_global_conf(&self.config);
        Ok(self)
    }

    pub fn reload_config(&mut self) -> Result<&mut Self, Error> {
        self.config = Config::default();
        self.load_config()?;
        self.emit_reconfigure()
    }

    fn apply_configured_paths(&mut self) {
        for directory in [
            "logs",
            "scripts",
            "public",
            "private",
            "databases",
            "plugins",
            "templates",
            "flowstreams",
            "modules",
            "services",
            "tmp",
        ] {
            let modern = format!("$dir{directory}");
            let legacy = format!("directory_{directory}");
            if let Some(path) = self
                .config
                .string(&modern)
                .or_else(|| self.config.string(&legacy))
            {
                self.paths.set_directory(directory, path);
            }
        }
        self.data = Data::new(self.paths.databases(None));
        self.filestorage = FileStorage::new(self.paths.private(Some("filestorage")));
    }

    pub fn prepare_directories(&self) -> Result<(), Error> {
        for path in [
            self.paths.logs(None),
            self.paths.tmp(None),
            self.paths.public(None),
            self.paths.private(None),
            self.paths.databases(None),
            self.paths.plugins(None),
            self.paths.modules(None),
            self.paths.services(None),
            self.paths.flowstreams(None),
        ] {
            self.paths.verify(&path).map_err(|err| {
                Error::internal(format!("failed to prepare {}: {err}", path.display()))
            })?;
        }
        Ok(())
    }

    pub fn install<I>(&mut self, module: I) -> Result<&mut Self, Error>
    where
        I: Install,
    {
        module.install(self)?;
        Ok(self)
    }

    pub fn install_many<I>(&mut self, modules: I) -> Result<&mut Self, Error>
    where
        I: IntoIterator<Item = fn(&mut Total) -> Result<(), Error>>,
    {
        for module in modules {
            self.install(module)?;
        }
        Ok(self)
    }

    pub fn group<F>(&mut self, prefix: &str, install: F) -> Result<&mut Self, Error>
    where
        F: FnOnce(&mut RouteGroup<'_>) -> Result<(), Error>,
    {
        let prefix = normalize_route_prefix(prefix)?;
        let mut group = RouteGroup { app: self, prefix };
        install(&mut group)?;
        Ok(self)
    }

    pub fn route<F, Fut>(&mut self, expression: &str, handler: F) -> Result<&mut Self, Error>
    where
        F: Fn(Context) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Response> + Send + 'static,
    {
        let (method, path, auth) = parse_route_expression(expression)?;
        self.add_route_with_auth(method, path, auth, handler)
    }

    pub fn route_compat(&mut self, expression: &str) -> Result<&mut Self, Error> {
        let route = CompatRoute::parse(expression)?;
        if let Some(api) = route.api {
            let base = resolve_api_base(&route.path, self.config.string("$api").unwrap_or("/api/"));
            let base = normalize_route_path(&base)?;
            self.api_routes.entry(base).or_default().insert(
                api.name,
                ApiEndpointDef {
                    action: route.action,
                    params: api.params,
                    auth: route.auth,
                },
            );
            return Ok(self);
        }
        let action = route.action.clone();
        self.add_route_with_auth(route.method, route.path, route.auth, move |ctx| {
            let action = action.clone();
            async move { ctx.action_success(&action, ctx.action_input()).await }
        })
    }

    pub fn add_route<F, Fut>(
        &mut self,
        method: RouteMethod,
        path: impl Into<String>,
        handler: F,
    ) -> Result<&mut Self, Error>
    where
        F: Fn(Context) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Response> + Send + 'static,
    {
        self.add_route_with_auth(method, path, RouteAuth::Any, handler)
    }

    fn add_route_with_auth<F, Fut>(
        &mut self,
        method: RouteMethod,
        path: impl Into<String>,
        auth: RouteAuth,
        handler: F,
    ) -> Result<&mut Self, Error>
    where
        F: Fn(Context) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Response> + Send + 'static,
    {
        let total_path = normalize_route_path(&path.into())?;
        let handler: Handler = Arc::new(move |ctx| Box::pin(handler(ctx)));
        self.routes.push(RouteDef {
            method,
            path: total_path,
            handler,
            auth,
        });
        Ok(self)
    }

    pub fn action<F, Fut>(&mut self, name: &str, action: F) -> &mut Self
    where
        F: Fn(Context, Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Value, Error>> + Send + 'static,
    {
        self.actions.insert(
            name.to_string(),
            Arc::new(move |ctx, value| Box::pin(action(ctx, value))),
        );
        self
    }

    pub fn action_options<F, Fut>(
        &mut self,
        name: &str,
        options: ActionOptions,
        action: F,
    ) -> &mut Self
    where
        F: Fn(Context, Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Value, Error>> + Send + 'static,
    {
        let action = Arc::new(action);
        self.action(name, move |ctx, input| {
            let options = options.clone();
            let action = action.clone();
            async move {
                let input = options.prepare(&ctx, input)?;
                action(ctx, input).await
            }
        });
        self
    }

    pub fn schema<F>(&mut self, name: &str, install: F) -> Result<&mut Self, Error>
    where
        F: FnOnce(&mut Schema<'_>) -> Result<(), Error>,
    {
        let name = normalize_schema_name(name)?;
        let mut schema = Schema { app: self, name };
        install(&mut schema)?;
        Ok(self)
    }

    pub fn middleware<F, Fut>(&mut self, middleware: F) -> &mut Self
    where
        F: Fn(Context) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Context, Response>> + Send + 'static,
    {
        self.middlewares
            .push(Arc::new(move |ctx| Box::pin(middleware(ctx))));
        self
    }

    pub fn auth<F, Fut>(&mut self, auth: F) -> &mut Self
    where
        F: Fn(Context) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Context, Response>> + Send + 'static,
    {
        self.auth = Some(Arc::new(move |ctx| Box::pin(auth(ctx))));
        self
    }

    pub fn websocket<F, Fut>(&mut self, path: &str, handler: F) -> Result<&mut Self, Error>
    where
        F: Fn(WsContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let total_path = normalize_route_path(path)?;
        self.websockets.push(WebSocketDef {
            path: total_path,
            handler: Arc::new(move |ctx| Box::pin(handler(ctx))),
            auth: RouteAuth::Any,
            protocols: Vec::new(),
        });
        Ok(self)
    }

    pub fn websocket_options<F, Fut>(
        &mut self,
        path: &str,
        protocols: &[&str],
        require_auth: bool,
        handler: F,
    ) -> Result<&mut Self, Error>
    where
        F: Fn(WsContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let total_path = normalize_route_path(path)?;
        self.websockets.push(WebSocketDef {
            path: total_path,
            handler: Arc::new(move |ctx| Box::pin(handler(ctx))),
            auth: if require_auth {
                RouteAuth::Member
            } else {
                RouteAuth::Any
            },
            protocols: protocols.iter().map(|value| value.to_string()).collect(),
        });
        Ok(self)
    }

    pub fn data(&self) -> &Data {
        &self.data
    }

    pub fn filestorage(&self) -> &FileStorage {
        &self.filestorage
    }

    pub fn plugin(&mut self, id: impl Into<String>, plugin: Plugin) -> &mut Self {
        self.plugins.insert(id.into(), plugin);
        self
    }

    pub fn plugins(&self) -> &HashMap<String, Plugin> {
        &self.plugins
    }

    pub fn route_count(&self) -> usize {
        self.routes.len()
    }

    pub fn action_count(&self) -> usize {
        self.actions.len()
    }

    pub fn middleware_count(&self) -> usize {
        self.middlewares.len()
    }

    pub fn plugin_count(&self) -> usize {
        self.plugins.len()
    }

    pub fn flowstream_count(&self) -> usize {
        self.flowstreams.len()
    }

    pub fn flowstream(&mut self, id: impl Into<String>) -> &mut FlowStream {
        let id = id.into();
        self.flowstreams
            .entry(id.clone())
            .or_insert_with(|| FlowStream::new(id))
    }

    pub fn on_load<F>(&mut self, hook: F) -> &mut Self
    where
        F: Fn(&mut Total) -> Result<(), Error> + Send + Sync + 'static,
    {
        self.lifecycle.load.push(Arc::new(hook));
        self
    }

    pub fn on_ready<F>(&mut self, hook: F) -> &mut Self
    where
        F: Fn(&mut Total) -> Result<(), Error> + Send + Sync + 'static,
    {
        self.lifecycle.ready.push(Arc::new(hook));
        self
    }

    pub fn on_reconfigure<F>(&mut self, hook: F) -> &mut Self
    where
        F: Fn(&mut Total) -> Result<(), Error> + Send + Sync + 'static,
    {
        self.lifecycle.reconfigure.push(Arc::new(hook));
        self
    }

    pub fn emit_load(&mut self) -> Result<&mut Self, Error> {
        let hooks = self.lifecycle.load.clone();
        for hook in hooks {
            hook(self)?;
        }
        Ok(self)
    }

    pub fn emit_ready(&mut self) -> Result<&mut Self, Error> {
        let hooks = self.lifecycle.ready.clone();
        for hook in hooks {
            hook(self)?;
        }
        Ok(self)
    }

    pub fn emit_reconfigure(&mut self) -> Result<&mut Self, Error> {
        let hooks = self.lifecycle.reconfigure.clone();
        for hook in hooks {
            hook(self)?;
        }
        Ok(self)
    }

    pub fn router(self) -> NativeDispatcher {
        self.native_dispatcher()
    }

    pub async fn run(self) -> Result<(), Error> {
        let addr: SocketAddr = format!("{}:{}", self.config.ip, self.config.port)
            .parse()
            .map_err(|err| Error::internal(format!("invalid listen address: {err}")))?;
        self.run_on(addr).await
    }

    pub async fn run_dev(mut self) -> Result<(), Error> {
        self.config.set("debug", true)?;
        self.config.set("mode", "development")?;
        self.run().await
    }

    pub async fn run_on(mut self, addr: SocketAddr) -> Result<(), Error> {
        self.emit_load()?;
        self.emit_ready()?;
        sync_global_conf(&self.config);
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "total5=info".into()),
            )
            .try_init()
            .ok();

        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .map_err(|err| Error::internal(format!("failed to bind {addr}: {err}")))?;
        self.print_startup(addr);
        let config = server::ServerConfig {
            limits: server::HttpLimits {
                max_header_bytes: 32 * 1024,
                max_body_bytes: self.config.body_limit,
            },
            handler_timeout: std::time::Duration::from_secs(self.config.handler_timeout),
            ..server::ServerConfig::default()
        };
        let tls_paths = self
            .config
            .string("https_cert")
            .or_else(|| self.config.string("tls_cert"))
            .zip(
                self.config
                    .string("https_key")
                    .or_else(|| self.config.string("tls_key")),
            )
            .map(|(certificate, key)| (certificate.to_string(), key.to_string()));
        let has_partial_tls_config = self.config.string("https_cert").is_some()
            || self.config.string("tls_cert").is_some()
            || self.config.string("https_key").is_some()
            || self.config.string("tls_key").is_some();
        if has_partial_tls_config && tls_paths.is_none() {
            return Err(Error::internal(
                "HTTPS requires both https_cert (or tls_cert) and https_key (or tls_key)",
            ));
        }
        let dispatcher = self.native_dispatcher();
        let handler = move |request| {
            let dispatcher = dispatcher.clone();
            async move { dispatcher.dispatch(request).await }
        };
        if let Some((certificate, key)) = tls_paths {
            let tls_config = server::load_tls_config(certificate, key)
                .map_err(|err| Error::internal(format!("TLS configuration error: {err:?}")))?;
            server::serve_tls(listener, tls_config, config, handler, async {
                let _ = tokio::signal::ctrl_c().await;
            })
            .await
        } else {
            server::serve(listener, config, handler, async {
                let _ = tokio::signal::ctrl_c().await;
            })
            .await
        }
        .map_err(|err| Error::internal(format!("server error: {err:?}")))
    }

    fn native_dispatcher(self) -> NativeDispatcher {
        let state = AppState {
            paths: self.paths,
            config: self.config,
            actions: Arc::new(self.actions),
            middlewares: Arc::new(self.middlewares),
            auth: self.auth,
            plugins: Arc::new(self.plugins),
            flowstreams: Arc::new(self.flowstreams),
            data: self.data,
            filestorage: self.filestorage,
            stats: self.stats,
            ws_connections: Arc::new(Mutex::new(HashMap::new())),
        };
        NativeDispatcher {
            state: Arc::new(state),
            routes: Arc::new(self.routes),
            api_routes: Arc::new(self.api_routes),
            websockets: Arc::new(self.websockets),
        }
    }

    fn print_startup(&self, addr: SocketAddr) {
        let separator = "====================================================";
        let mode = if self
            .config
            .get("debug")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            "debug"
        } else {
            "release"
        };
        let memory = memory_usage()
            .map(format_filesize)
            .unwrap_or_else(|| "n/a".to_string());
        let user = std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .unwrap_or_else(|_| "unknown".to_string());
        let tz = std::env::var("TZ").unwrap_or_else(|_| "UTC".to_string());
        let os = os_description();
        let directory = display_directory(&self.root);
        let crate_directory = display_directory(Path::new(env!("CARGO_MANIFEST_DIR")));
        let components = self.route_count()
            + self.websockets.len()
            + self.action_count()
            + self.middleware_count()
            + self.plugin_count()
            + self.flowstream_count();

        println!("{separator}");
        println!("PID           : {}", std::process::id());
        println!("Rust          : {}", env!("TOTAL5_RUSTC_VERSION"));
        println!("Total.rs      : v{VERSION}");
        println!("OS            : {os}");
        println!("Memory        : {memory}");
        println!("User          : {user}");
        println!("{separator}");
        println!("Name          : {}", self.config.name);
        println!("Version       : {}", self.config.version);
        println!(
            "Date ({tz})    : {}",
            Utc::now().format("%Y-%m-%d %H:%M:%S")
        );
        println!("Mode          : {mode}");
        println!("Compiled      : {components} components (0ms)");
        println!("{separator}");
        println!("Directory     : {directory}");
        println!("total5        : {crate_directory}");
        println!("{separator}\n");
        println!("http://{addr}/");
        println!();
    }
}

impl NativeDispatcher {
    pub fn statistics(&self) -> StatisticsSnapshot {
        self.state.stats.snapshot()
    }

    pub async fn oneshot<B>(
        &self,
        request: http::Request<B>,
    ) -> Result<http::Response<Bytes>, std::convert::Infallible>
    where
        B: Into<Bytes>,
    {
        let (parts, body) = request.into_parts();
        let request = http::Request::from_parts(parts, body.into());
        Ok(self.dispatch(request).await)
    }

    async fn dispatch(&self, request: http::Request<Bytes>) -> http::Response<Bytes> {
        let stats = self.state.stats.clone();
        stats.requests.fetch_add(1, Ordering::Relaxed);
        stats
            .downloaded_bytes
            .fetch_add(request.body().len() as u64, Ordering::Relaxed);
        let ip = request_ip(request.headers());
        if ip_is_blacklisted(
            self.state.config.string("$blacklist").unwrap_or_default(),
            &ip,
        ) {
            stats.blocked.fetch_add(1, Ordering::Relaxed);
            stats.responses_4xx.fetch_add(1, Ordering::Relaxed);
            return server::error_response(StatusCode::FORBIDDEN, "request blocked");
        }
        let limit = self
            .state
            .config
            .get("$httpreqlimit")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        if limit > 0 && request_limit_exceeded(&stats, &ip, limit) {
            stats.throttled.fetch_add(1, Ordering::Relaxed);
            stats.responses_4xx.fetch_add(1, Ordering::Relaxed);
            return http::Response::builder()
                .status(StatusCode::TOO_MANY_REQUESTS)
                .header(header::RETRY_AFTER, "60")
                .body(Bytes::from_static(b"too many requests"))
                .expect("valid throttling response");
        }
        stats.pending.fetch_add(1, Ordering::Relaxed);
        let _pending = PendingRequest(stats.clone());
        let response = self.dispatch_inner(request).await;
        stats
            .uploaded_bytes
            .fetch_add(response.body().len() as u64, Ordering::Relaxed);
        match response.status().as_u16() {
            200..=299 => {
                stats.responses_2xx.fetch_add(1, Ordering::Relaxed);
            }
            400..=499 => {
                stats.responses_4xx.fetch_add(1, Ordering::Relaxed);
            }
            500..=599 => {
                stats.responses_5xx.fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }
        if response.status() == StatusCode::SERVICE_UNAVAILABLE
            && response.body().as_ref() == b"request timeout"
        {
            stats.timeouts.fetch_add(1, Ordering::Relaxed);
        }
        response
    }

    async fn dispatch_inner(&self, request: http::Request<Bytes>) -> http::Response<Bytes> {
        let (parts, body) = request.into_parts();
        let method = parts.method;
        let uri = parts.uri;
        let headers = parts.headers;
        let response_headers = headers.clone();
        let query = parse_urlencoded(uri.query().unwrap_or_default());

        if is_websocket_upgrade(&method, &headers) {
            return self.websocket_upgrade(uri, headers).await;
        }

        if method == Method::OPTIONS {
            let response = self.options_response(uri.path());
            return finalize_cors(response, &headers, &self.state.config, true);
        }
        let route_method = if method == Method::HEAD {
            Method::GET
        } else {
            method.clone()
        };

        if method == Method::POST {
            if let Some((_, endpoints)) = self
                .api_routes
                .iter()
                .find(|(path, _)| paths_equal(path, uri.path()))
            {
                let response = dispatch_api_native(
                    self.state.clone(),
                    query,
                    uri,
                    headers,
                    body,
                    Arc::new(endpoints.clone()),
                )
                .await;
                return finalize_http_response(
                    response.into_http_response(),
                    &method,
                    &response_headers,
                    &self.state.config,
                );
            }
        }

        for route in self.routes.iter() {
            if route.method.into_http_method() != route_method {
                continue;
            }
            let Some(params) = match_native_path(&route.path, uri.path()) else {
                continue;
            };
            let response = dispatch_native(
                self.state.clone(),
                params,
                query,
                uri,
                headers,
                body,
                route.handler.clone(),
                method.clone(),
                route.auth,
            )
            .await;
            return finalize_http_response(
                response.into_http_response(),
                &method,
                &response_headers,
                &self.state.config,
            );
        }

        let response = if method == Method::GET || method == Method::HEAD {
            match static_file_response(&self.state, uri.path()).await {
                Ok(Some(response)) => response,
                Ok(None) => Error::not_found("route not found").into_response(),
                Err(err) => err.into_response(),
            }
        } else {
            Error::not_found("route not found").into_response()
        };
        finalize_http_response(
            response.into_http_response(),
            &method,
            &response_headers,
            &self.state.config,
        )
    }

    fn options_response(&self, path: &str) -> http::Response<Bytes> {
        let mut methods: Vec<String> = Vec::new();
        for route in self.routes.iter() {
            if match_native_path(&route.path, path).is_none() {
                continue;
            }
            let method = route.method.into_http_method().as_str().to_string();
            if !methods.contains(&method) {
                methods.push(method);
            }
            if route.method == RouteMethod::Get && !methods.iter().any(|value| value == "HEAD") {
                methods.push("HEAD".to_string());
            }
        }
        if self.api_routes.keys().any(|base| paths_equal(base, path))
            && !methods.iter().any(|value| value == "POST")
        {
            methods.push("POST".to_string());
        }
        if methods.is_empty() {
            return Error::not_found("route not found")
                .into_response()
                .into_http_response();
        }
        methods.push("OPTIONS".to_string());
        http::Response::builder()
            .status(StatusCode::NO_CONTENT)
            .header(header::ALLOW, methods.join(", "))
            .body(Bytes::new())
            .expect("valid OPTIONS response")
    }

    async fn websocket_upgrade(&self, uri: Uri, headers: HeaderMap) -> http::Response<Bytes> {
        let path = uri.path();
        let Some(route) = self
            .websockets
            .iter()
            .find(|route| match_native_path(&route.path, path).is_some())
        else {
            return Error::not_found("websocket route not found")
                .into_response()
                .into_http_response();
        };
        let params = match_native_path(&route.path, path).unwrap_or_default();
        let query = parse_urlencoded(uri.query().unwrap_or_default());
        if headers
            .get("sec-websocket-version")
            .and_then(|value| value.to_str().ok())
            != Some("13")
        {
            return http::Response::builder()
                .status(StatusCode::UPGRADE_REQUIRED)
                .header("sec-websocket-version", "13")
                .body(Bytes::new())
                .expect("valid websocket version response");
        }
        let Some(key) = headers
            .get("sec-websocket-key")
            .and_then(|value| value.to_str().ok())
        else {
            return Error::bad_request("missing websocket key")
                .into_response()
                .into_http_response();
        };
        let accept = websocket_accept(key);
        let compression = self.state.config.boolean("$wscompress").unwrap_or(true)
            && headers
                .get("sec-websocket-extensions")
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| {
                    value
                        .split(',')
                        .any(|extension| extension.trim().starts_with("permessage-deflate"))
                });
        let protocol = headers
            .get("sec-websocket-protocol")
            .and_then(|value| value.to_str().ok())
            .and_then(|requested| {
                requested
                    .split(',')
                    .map(str::trim)
                    .find(|requested| route.protocols.iter().any(|allowed| allowed == requested))
                    .map(str::to_string)
            });
        if !route.protocols.is_empty() && protocol.is_none() {
            return Error::bad_request("no supported websocket subprotocol")
                .into_response()
                .into_http_response();
        }
        let mut auth_context = Context {
            state: self.state.clone(),
            method: Method::GET,
            uri: uri.clone(),
            params: params.clone(),
            query: query.clone(),
            headers: headers.clone(),
            body: Bytes::new(),
            fields: HashMap::new(),
            files: Vec::new(),
            data: HashMap::new(),
            user: None,
            model: Value::Null,
        };
        if let Some(auth) = self.state.auth.clone() {
            auth_context = match auth(auth_context).await {
                Ok(context) => context,
                Err(response) => return response.into_http_response(),
            };
        }
        for middleware in self.state.middlewares.iter() {
            auth_context = match middleware(auth_context).await {
                Ok(context) => context,
                Err(response) => return response.into_http_response(),
            };
        }
        if route.auth == RouteAuth::Member && !auth_context.is_authenticated() {
            return Error {
                status: StatusCode::UNAUTHORIZED,
                message: "Unauthorized".to_string(),
                validation: Vec::new(),
            }
            .into_response()
            .into_http_response();
        }
        let (incoming_tx, incoming_rx) = tokio::sync::mpsc::channel(32);
        let (outgoing_tx, outgoing_rx) = tokio::sync::mpsc::channel(32);
        let connection_id = uuid::Uuid::new_v4().to_string();
        self.state
            .ws_connections
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(connection_id.clone(), outgoing_tx.clone());
        let context = WsContext {
            state: self.state.clone(),
            incoming: incoming_rx,
            outgoing: outgoing_tx,
            uri,
            headers,
            params,
            query,
            user: auth_context.user,
            protocol: protocol.clone(),
            connection_id: connection_id.clone(),
        };
        let handler = route.handler.clone();
        let stats = self.state.stats.clone();
        let connections = self.state.ws_connections.clone();
        stats.websocket_connections.fetch_add(1, Ordering::Relaxed);
        tokio::spawn(async move {
            handler(context).await;
            connections
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .remove(&connection_id);
            stats.websocket_connections.fetch_sub(1, Ordering::Relaxed);
        });

        let mut response = http::Response::builder()
            .status(StatusCode::SWITCHING_PROTOCOLS)
            .header(header::UPGRADE, "websocket")
            .header(header::CONNECTION, "Upgrade")
            .header("sec-websocket-accept", accept)
            .body(Bytes::new())
            .expect("valid websocket upgrade response");
        if let Some(protocol) = protocol {
            response.headers_mut().insert(
                http::HeaderName::from_static("sec-websocket-protocol"),
                header::HeaderValue::from_str(&protocol).expect("validated websocket protocol"),
            );
        }
        if compression {
            response.headers_mut().insert(
                http::HeaderName::from_static("sec-websocket-extensions"),
                header::HeaderValue::from_static(
                    "permessage-deflate; server_no_context_takeover; client_no_context_takeover",
                ),
            );
        }
        response.extensions_mut().insert(
            server::WebSocketSession::new(incoming_tx, outgoing_rx, self.state.config.body_limit)
                .with_compression(compression),
        );
        response
    }
}

fn finalize_head(mut response: http::Response<Bytes>, head: bool) -> http::Response<Bytes> {
    if head {
        if !response.headers().contains_key(header::CONTENT_LENGTH) {
            let length = header::HeaderValue::from_str(&response.body().len().to_string())
                .expect("response length is a valid header");
            response
                .headers_mut()
                .insert(header::CONTENT_LENGTH, length);
        }
        *response.body_mut() = Bytes::new();
    }
    response
}

fn finalize_http_response(
    mut response: http::Response<Bytes>,
    method: &Method,
    request_headers: &HeaderMap,
    config: &Config,
) -> http::Response<Bytes> {
    response = finalize_cors(response, request_headers, config, false);
    if response
        .headers()
        .get(header::ACCEPT_RANGES)
        .is_some_and(|value| value == "bytes")
    {
        if let Some(range) = request_headers
            .get(header::RANGE)
            .and_then(|value| value.to_str().ok())
        {
            apply_byte_range(&mut response, range);
        }
    }

    let accepts_gzip = request_headers
        .get(header::ACCEPT_ENCODING)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value.split(',').any(|item| {
                item.trim()
                    .split(';')
                    .next()
                    .is_some_and(|encoding| encoding.eq_ignore_ascii_case("gzip"))
            })
        });
    let compress = config.boolean("$httpcompress").unwrap_or(true)
        && accepts_gzip
        && response.status() == StatusCode::OK
        && response.body().len() > 256
        && !response.headers().contains_key(header::CONTENT_ENCODING)
        && !response.headers().contains_key(header::CONTENT_RANGE)
        && response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(is_compressible_content_type);
    if compress {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        if encoder.write_all(response.body()).is_ok() {
            if let Ok(compressed) = encoder.finish() {
                *response.body_mut() = Bytes::from(compressed);
                response.headers_mut().insert(
                    header::CONTENT_ENCODING,
                    header::HeaderValue::from_static("gzip"),
                );
                append_vary(response.headers_mut(), "Accept-Encoding");
                response.headers_mut().remove(header::CONTENT_LENGTH);
            }
        }
    }
    finalize_head(response, method == Method::HEAD)
}

fn finalize_cors(
    mut response: http::Response<Bytes>,
    request_headers: &HeaderMap,
    config: &Config,
    preflight: bool,
) -> http::Response<Bytes> {
    let Some(origin) = request_headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    else {
        return response;
    };
    let configured = config.string("$cors").unwrap_or_default();
    let allowed = cors_allowed_origin(configured, origin);
    if allowed.is_none() {
        if preflight && !configured.trim().is_empty() {
            *response.status_mut() = StatusCode::FORBIDDEN;
            *response.body_mut() = Bytes::from_static(b"CORS origin denied");
        }
        return response;
    }
    let allow_origin = if configured.trim() == "*" {
        "*"
    } else {
        origin
    };
    if let Ok(value) = header::HeaderValue::from_str(allow_origin) {
        response
            .headers_mut()
            .insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, value);
    }
    append_vary(response.headers_mut(), "Origin");
    if preflight {
        if let Some(method) = request_headers
            .get(header::ACCESS_CONTROL_REQUEST_METHOD)
            .cloned()
        {
            response
                .headers_mut()
                .insert(header::ACCESS_CONTROL_ALLOW_METHODS, method);
        } else if let Some(allow) = response.headers().get(header::ALLOW).cloned() {
            response
                .headers_mut()
                .insert(header::ACCESS_CONTROL_ALLOW_METHODS, allow);
        }
        if let Some(headers) = request_headers
            .get(header::ACCESS_CONTROL_REQUEST_HEADERS)
            .cloned()
        {
            response
                .headers_mut()
                .insert(header::ACCESS_CONTROL_ALLOW_HEADERS, headers);
        }
        response.headers_mut().insert(
            header::ACCESS_CONTROL_MAX_AGE,
            header::HeaderValue::from_static("600"),
        );
    }
    response
}

fn cors_allowed_origin(configured: &str, origin: &str) -> Option<()> {
    if configured.trim() == "*" {
        return Some(());
    }
    let origin = origin.to_ascii_lowercase();
    configured
        .split([',', ';', '|'])
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .any(|value| {
            let value = value.to_ascii_lowercase();
            value == origin
                || value
                    .strip_prefix('*')
                    .is_some_and(|suffix| origin.ends_with(suffix))
        })
        .then_some(())
}

fn append_vary(headers: &mut HeaderMap, value: &str) {
    let existing = headers
        .get(header::VARY)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if existing
        .split(',')
        .any(|item| item.trim().eq_ignore_ascii_case(value))
    {
        return;
    }
    let combined = if existing.is_empty() {
        value.to_string()
    } else {
        format!("{existing}, {value}")
    };
    if let Ok(value) = header::HeaderValue::from_str(&combined) {
        headers.insert(header::VARY, value);
    }
}

fn is_compressible_content_type(content_type: &str) -> bool {
    let content_type = content_type
        .split(';')
        .next()
        .unwrap_or(content_type)
        .trim();
    content_type.starts_with("text/")
        || matches!(
            content_type,
            "application/json" | "application/javascript" | "application/xml" | "image/svg+xml"
        )
}

fn apply_byte_range(response: &mut http::Response<Bytes>, value: &str) {
    let total = response.body().len();
    let Some(specification) = value.strip_prefix("bytes=") else {
        return;
    };
    if specification.contains(',') {
        range_not_satisfiable(response, total);
        return;
    }
    let Some((start, end)) = specification.trim().split_once('-') else {
        range_not_satisfiable(response, total);
        return;
    };
    let range = if start.is_empty() {
        end.parse::<usize>()
            .ok()
            .filter(|length| *length > 0)
            .map(|length| {
                let length = length.min(total);
                (total.saturating_sub(length), total.saturating_sub(1))
            })
    } else {
        start.parse::<usize>().ok().and_then(|start| {
            if start >= total {
                return None;
            }
            let end = if end.is_empty() {
                total - 1
            } else {
                end.parse::<usize>().ok()?.min(total - 1)
            };
            (start <= end).then_some((start, end))
        })
    };
    let Some((start, end)) = range else {
        range_not_satisfiable(response, total);
        return;
    };
    let body = response.body().slice(start..=end);
    *response.status_mut() = StatusCode::PARTIAL_CONTENT;
    response.headers_mut().insert(
        header::CONTENT_RANGE,
        header::HeaderValue::from_str(&format!("bytes {start}-{end}/{total}"))
            .expect("valid content range"),
    );
    response.headers_mut().insert(
        header::CONTENT_LENGTH,
        header::HeaderValue::from_str(&body.len().to_string()).expect("valid range length"),
    );
    *response.body_mut() = body;
}

fn range_not_satisfiable(response: &mut http::Response<Bytes>, total: usize) {
    *response.status_mut() = StatusCode::RANGE_NOT_SATISFIABLE;
    response.headers_mut().insert(
        header::CONTENT_RANGE,
        header::HeaderValue::from_str(&format!("bytes */{total}")).expect("valid content range"),
    );
    response.headers_mut().insert(
        header::CONTENT_LENGTH,
        header::HeaderValue::from_static("0"),
    );
    *response.body_mut() = Bytes::new();
}

fn match_native_path(pattern: &str, path: &str) -> Option<HashMap<String, String>> {
    let pattern = pattern.trim_matches('/');
    let path = path.trim_matches('/');
    let pattern_parts = if pattern.is_empty() {
        Vec::new()
    } else {
        pattern.split('/').collect::<Vec<_>>()
    };
    let path_parts = if path.is_empty() {
        Vec::new()
    } else {
        path.split('/').collect::<Vec<_>>()
    };
    if pattern_parts.len() != path_parts.len() {
        return None;
    }
    let mut params = HashMap::new();
    for (expected, actual) in pattern_parts.into_iter().zip(path_parts) {
        if let Some(name) = expected
            .strip_prefix('{')
            .and_then(|value| value.strip_suffix('}'))
        {
            if name.is_empty() {
                return None;
            }
            params.insert(name.to_string(), percent_decode(actual));
        } else if expected != actual {
            return None;
        }
    }
    Some(params)
}

fn paths_equal(left: &str, right: &str) -> bool {
    left.trim_matches('/') == right.trim_matches('/')
}

fn is_websocket_upgrade(method: &Method, headers: &HeaderMap) -> bool {
    if method != Method::GET {
        return false;
    }
    let upgrade = headers
        .get(header::UPGRADE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("websocket"));
    let connection = headers
        .get(header::CONNECTION)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .split(',')
        .any(|value| value.trim().eq_ignore_ascii_case("upgrade"));
    upgrade && connection
}

pub(crate) fn websocket_accept(key: &str) -> String {
    let mut input = key.trim().as_bytes().to_vec();
    input.extend_from_slice(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
    base64_encode(&sha1(&input))
}

fn sha1(input: &[u8]) -> [u8; 20] {
    let bit_length = (input.len() as u64) * 8;
    let mut message = input.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_length.to_be_bytes());
    let mut state = [
        0x67452301_u32,
        0xEFCDAB89,
        0x98BADCFE,
        0x10325476,
        0xC3D2E1F0,
    ];
    for block in message.chunks_exact(64) {
        let mut words = [0_u32; 80];
        for (index, chunk) in block.chunks_exact(4).enumerate() {
            words[index] = u32::from_be_bytes(chunk.try_into().expect("four-byte SHA-1 word"));
        }
        for index in 16..80 {
            words[index] =
                (words[index - 3] ^ words[index - 8] ^ words[index - 14] ^ words[index - 16])
                    .rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = state;
        for (index, word) in words.into_iter().enumerate() {
            let (function, constant) = match index {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let next = a
                .rotate_left(5)
                .wrapping_add(function)
                .wrapping_add(e)
                .wrapping_add(constant)
                .wrapping_add(word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = next;
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
    }
    let mut output = [0_u8; 20];
    for (index, word) in state.into_iter().enumerate() {
        output[index * 4..index * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    output
}

fn base64_encode(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let value = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        output.push(TABLE[((value >> 18) & 63) as usize] as char);
        output.push(TABLE[((value >> 12) & 63) as usize] as char);
        output.push(if chunk.len() > 1 {
            TABLE[((value >> 6) & 63) as usize] as char
        } else {
            '='
        });
        output.push(if chunk.len() > 2 {
            TABLE[(value & 63) as usize] as char
        } else {
            '='
        });
    }
    output
}

fn display_directory(path: &Path) -> String {
    let mut value = path.display().to_string();
    if !value.ends_with(std::path::MAIN_SEPARATOR) {
        value.push(std::path::MAIN_SEPARATOR);
    }
    value
}

fn os_description() -> String {
    let release = stdfs::read_to_string("/proc/sys/kernel/osrelease")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    match release {
        Some(release) => format!("{} {release}", std::env::consts::OS),
        None => format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
    }
}

fn memory_usage() -> Option<u64> {
    let status = stdfs::read_to_string("/proc/self/status").ok()?;
    for line in status.lines() {
        let Some(value) = line.strip_prefix("VmRSS:") else {
            continue;
        };
        let kb = value
            .split_whitespace()
            .next()
            .and_then(|value| value.parse::<u64>().ok())?;
        return Some(kb * 1024);
    }
    None
}

fn format_filesize(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", bytes, UNITS[unit])
    } else {
        format!("{value:.2} {}", UNITS[unit])
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Plugin {
    pub name: String,
    pub icon: Option<String>,
    pub group: Option<String>,
    pub position: i32,
    pub permissions: Vec<Permission>,
}

impl Plugin {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Self::default()
        }
    }

    pub fn icon(mut self, icon: impl Into<String>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn group(mut self, group: impl Into<String>) -> Self {
        self.group = Some(group.into());
        self
    }

    pub fn position(mut self, position: i32) -> Self {
        self.position = position;
        self
    }

    pub fn permission(mut self, id: impl Into<String>, name: impl Into<String>) -> Self {
        self.permissions.push(Permission {
            id: id.into(),
            name: name.into(),
        });
        self
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Permission {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Default)]
struct Lifecycle {
    load: Vec<LifecycleHook>,
    ready: Vec<LifecycleHook>,
    reconfigure: Vec<LifecycleHook>,
}

#[derive(Clone)]
pub struct FlowStream {
    pub id: String,
    pub name: String,
    inputs: HashMap<String, FlowHandler>,
    rpcs: HashMap<String, FlowHandler>,
}

impl FlowStream {
    pub fn new(id: impl Into<String>) -> Self {
        let id = id.into();
        Self {
            name: id.clone(),
            id,
            inputs: HashMap::new(),
            rpcs: HashMap::new(),
        }
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    pub fn input<F, Fut>(&mut self, name: &str, handler: F) -> &mut Self
    where
        F: Fn(FlowMessage) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Value, Error>> + Send + 'static,
    {
        self.inputs.insert(
            name.to_string(),
            Arc::new(move |msg| Box::pin(handler(msg))),
        );
        self
    }

    pub fn rpc<F, Fut>(&mut self, name: &str, handler: F) -> &mut Self
    where
        F: Fn(FlowMessage) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Value, Error>> + Send + 'static,
    {
        self.rpcs.insert(
            name.to_string(),
            Arc::new(move |msg| Box::pin(handler(msg))),
        );
        self
    }

    async fn input_exec(&self, name: &str, data: Value) -> Result<Value, Error> {
        let handler = self.inputs.get(name).cloned().ok_or_else(|| {
            Error::not_found(format!("flow input not found: {}/{}", self.id, name))
        })?;
        handler(FlowMessage::new(self.id.clone(), name.to_string(), data)).await
    }

    async fn rpc_exec(&self, name: &str, data: Value) -> Result<Value, Error> {
        let handler =
            self.rpcs.get(name).cloned().ok_or_else(|| {
                Error::not_found(format!("flow rpc not found: {}/{}", self.id, name))
            })?;
        handler(FlowMessage::new(self.id.clone(), name.to_string(), data)).await
    }
}

#[derive(Clone)]
pub struct FlowMessage {
    pub stream: String,
    pub name: String,
    pub data: Value,
}

impl FlowMessage {
    pub fn new(stream: String, name: String, data: Value) -> Self {
        Self { stream, name, data }
    }

    pub fn get(&self, name: &str) -> Option<&Value> {
        self.data.get(name)
    }

    pub fn string(&self, name: &str) -> Option<&str> {
        self.get(name).and_then(Value::as_str)
    }
}

/// Named Total-style schema for registering validated business actions.
pub struct Schema<'a> {
    app: &'a mut Total,
    name: String,
}

impl Schema<'_> {
    pub fn action<F, Fut>(&mut self, name: &str, action: F) -> &mut Self
    where
        F: Fn(Context, Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Value, Error>> + Send + 'static,
    {
        let name = format!("{}/{}", self.name, normalize_action_name(name));
        self.app.action(&name, action);
        self
    }

    pub fn action_with<F, Fut>(&mut self, name: &str, validator: Validator, action: F) -> &mut Self
    where
        F: Fn(Context, Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Value, Error>> + Send + 'static,
    {
        let name = format!("{}/{}", self.name, normalize_action_name(name));
        let action = Arc::new(action);
        self.app.action(&name, move |ctx, input| {
            let validator = validator.clone();
            let action = action.clone();
            async move {
                validator.validate(&input)?;
                action(ctx, input).await
            }
        });
        self
    }

    pub fn action_options<F, Fut>(
        &mut self,
        name: &str,
        options: ActionOptions,
        action: F,
    ) -> &mut Self
    where
        F: Fn(Context, Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Value, Error>> + Send + 'static,
    {
        let name = format!("{}/{}", self.name, normalize_action_name(name));
        let action = Arc::new(action);
        self.app.action(&name, move |ctx, input| {
            let options = options.clone();
            let action = action.clone();
            async move {
                let input = options.prepare(&ctx, input)?;
                action(ctx, input).await
            }
        });
        self
    }
}

#[derive(Clone, Default)]
pub struct ActionOptions {
    pub name: Option<String>,
    input: Option<Validator>,
    params: Option<Validator>,
    query: Option<Validator>,
}

impl ActionOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn input(mut self, fields: &str) -> Result<Self, Error> {
        self.input = Some(Validator::parse(fields)?);
        Ok(self)
    }

    pub fn params(mut self, fields: &str) -> Result<Self, Error> {
        self.params = Some(Validator::parse(fields)?);
        Ok(self)
    }

    pub fn query(mut self, fields: &str) -> Result<Self, Error> {
        self.query = Some(Validator::parse(fields)?);
        Ok(self)
    }

    fn prepare(&self, ctx: &Context, input: Value) -> Result<Value, Error> {
        if let Some(validator) = &self.params {
            validator.validate(&map_to_value(&ctx.params))?;
        }
        if let Some(validator) = &self.query {
            validator.validate(&map_to_value(&ctx.query))?;
        }
        if let Some(validator) = &self.input {
            validator.transform(input)
        } else {
            Ok(input)
        }
    }
}

/// Reusable schema validator.
#[derive(Clone, Default)]
pub struct Validator {
    fields: Vec<FieldRule>,
}

impl Validator {
    pub fn new() -> Self {
        Self { fields: Vec::new() }
    }

    pub fn field(mut self, name: &str, kind: FieldKind) -> Self {
        self.fields.push(FieldRule::new(name, kind));
        self
    }

    pub fn required(mut self, name: &str, kind: FieldKind) -> Self {
        self.fields.push(FieldRule::new(name, kind).required());
        self
    }

    pub fn rule(mut self, rule: FieldRule) -> Self {
        self.fields.push(rule);
        self
    }

    pub fn parse(fields: &str) -> Result<Self, Error> {
        let mut validator = Self::new();
        for token in split_schema_fields(fields) {
            let token = token.trim();
            if token.is_empty() {
                continue;
            }
            validator = validator.rule(parse_field_rule(token)?);
        }
        Ok(validator)
    }

    pub fn validate(&self, input: &Value) -> Result<(), Error> {
        let object = match input.as_object() {
            Some(object) => object,
            None => {
                return Err(Error::validation(vec![ValidationError::new(
                    "",
                    "object",
                    "expected object input",
                )]));
            }
        };

        let mut errors = Vec::new();
        for rule in &self.fields {
            rule.validate(object, &mut errors);
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(Error::validation(errors))
        }
    }

    pub fn transform(&self, input: Value) -> Result<Value, Error> {
        let object = match input.as_object() {
            Some(object) => object,
            None => {
                return Err(Error::validation(vec![ValidationError::new(
                    "",
                    "object",
                    "expected object input",
                )]));
            }
        };

        let mut errors = Vec::new();
        let mut output = object.clone();
        for rule in &self.fields {
            rule.validate(object, &mut errors);
            if errors.is_empty() {
                if let Some(value) = object.get(&rule.name) {
                    output.insert(rule.name.clone(), rule.transform_value(value));
                }
            }
        }

        if errors.is_empty() {
            Ok(Value::Object(output))
        } else {
            Err(Error::validation(errors))
        }
    }
}

#[derive(Clone)]
pub struct FieldRule {
    name: String,
    kind: FieldKind,
    required: bool,
    min: Option<usize>,
    max: Option<usize>,
    allowed: Option<Vec<Value>>,
    custom: Option<(String, CustomValidator)>,
    nested: Option<Box<Validator>>,
    array_kind: Option<FieldKind>,
    array_nested: Option<Box<Validator>>,
}

impl FieldRule {
    pub fn new(name: &str, kind: FieldKind) -> Self {
        Self {
            name: name.to_string(),
            kind,
            required: false,
            min: None,
            max: None,
            allowed: None,
            custom: None,
            nested: None,
            array_kind: None,
            array_nested: None,
        }
    }

    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    pub fn min(mut self, min: usize) -> Self {
        self.min = Some(min);
        self
    }

    pub fn max(mut self, max: usize) -> Self {
        self.max = Some(max);
        self
    }

    pub fn one_of<T>(mut self, values: Vec<T>) -> Self
    where
        T: Serialize,
    {
        self.allowed = Some(
            values
                .into_iter()
                .filter_map(|value| serde_json::to_value(value).ok())
                .collect(),
        );
        self
    }

    pub fn custom<F>(mut self, code: &str, validator: F) -> Self
    where
        F: Fn(&Value) -> bool + Send + Sync + 'static,
    {
        self.custom = Some((code.to_string(), Arc::new(validator)));
        self
    }

    pub fn nested(mut self, validator: Validator) -> Self {
        self.nested = Some(Box::new(validator));
        self
    }

    pub fn array_kind(mut self, kind: FieldKind) -> Self {
        self.array_kind = Some(kind);
        self
    }

    pub fn array_nested(mut self, validator: Validator) -> Self {
        self.array_nested = Some(Box::new(validator));
        self
    }

    fn validate(&self, object: &Map<String, Value>, errors: &mut Vec<ValidationError>) {
        let value = object.get(&self.name);
        if value.is_none() || value == Some(&Value::Null) {
            if self.required {
                errors.push(ValidationError::new(
                    &self.name,
                    "required",
                    "field is required",
                ));
            }
            return;
        }

        let value = value.unwrap();
        if !self.kind.matches(value) {
            errors.push(ValidationError::new(
                &self.name,
                self.kind.code(),
                self.kind.message(),
            ));
            return;
        }

        if let Some(nested) = &self.nested {
            if let Err(err) = nested.validate(value) {
                for item in err.validation {
                    errors.push(ValidationError::new(
                        format!("{}.{}", self.name, item.field),
                        item.error,
                        item.message,
                    ));
                }
                return;
            }
        }

        if let Some(array_kind) = self.array_kind {
            if let Some(items) = value.as_array() {
                for (index, item) in items.iter().enumerate() {
                    if !array_kind.matches(item) {
                        errors.push(ValidationError::new(
                            format!("{}[{index}]", self.name),
                            array_kind.code(),
                            array_kind.message(),
                        ));
                    }
                }
            }
        }

        if let Some(array_nested) = &self.array_nested {
            if let Some(items) = value.as_array() {
                for (index, item) in items.iter().enumerate() {
                    if let Err(err) = array_nested.validate(item) {
                        for child in err.validation {
                            errors.push(ValidationError::new(
                                format!("{}[{index}].{}", self.name, child.field),
                                child.error,
                                child.message,
                            ));
                        }
                    }
                }
            }
        }

        if let Some(min) = self.min {
            if value_length(value).is_some_and(|length| length < min) {
                errors.push(ValidationError::new(
                    &self.name,
                    "min",
                    format!("minimum length is {min}"),
                ));
            }
        }

        if let Some(max) = self.max {
            if value_length(value).is_some_and(|length| length > max) {
                errors.push(ValidationError::new(
                    &self.name,
                    "max",
                    format!("maximum length is {max}"),
                ));
            }
        }

        if let Some(allowed) = &self.allowed {
            if !allowed.iter().any(|item| item == value) {
                errors.push(ValidationError::new(
                    &self.name,
                    "enum",
                    "value is not allowed",
                ));
            }
        }

        if let Some((code, custom)) = &self.custom {
            if !custom(value) {
                errors.push(ValidationError::new(
                    &self.name,
                    code.clone(),
                    "custom validation failed",
                ));
            }
        }
    }

    fn transform_value(&self, value: &Value) -> Value {
        self.kind.transform(value)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum FieldKind {
    Any,
    String,
    Number,
    Bool,
    Array,
    Object,
    Email,
    Uid,
    Date,
    Phone,
    Lower,
    Lowercase,
    Uppercase,
    Capitalize,
    Capitalize2,
    Name,
    Base64,
    Url,
    Json,
    DataUri,
    Zip,
    Icon,
    Color,
    Guid,
    TinyInt,
    SmallInt,
}

impl FieldKind {
    fn matches(self, value: &Value) -> bool {
        match self {
            Self::Any => true,
            Self::String => value.is_string(),
            Self::Number => {
                value.is_number()
                    || value
                        .as_str()
                        .is_some_and(|value| value.parse::<f64>().is_ok())
            }
            Self::Bool => {
                value.is_boolean()
                    || value.as_str().is_some_and(|value| {
                        matches!(
                            value.to_ascii_lowercase().as_str(),
                            "true" | "false" | "1" | "0"
                        )
                    })
            }
            Self::Array => value.is_array(),
            Self::Object => value.is_object(),
            Self::Email => value.as_str().is_some_and(is_email),
            Self::Uid => value.as_str().is_some_and(|value| !value.trim().is_empty()),
            Self::Date => value.is_string() || value.is_number(),
            Self::Phone => value.as_str().is_some_and(|value| !value.trim().is_empty()),
            Self::Lower => value.is_string(),
            Self::Lowercase => value.is_string(),
            Self::Uppercase => value.is_string(),
            Self::Capitalize => value.is_string(),
            Self::Capitalize2 => value.is_string(),
            Self::Name => value.is_string(),
            Self::Base64 => value.as_str().is_some_and(is_base64),
            Self::Url => value.as_str().is_some_and(is_url),
            Self::Json => {
                value.is_object()
                    || value.is_array()
                    || value
                        .as_str()
                        .is_some_and(|value| serde_json::from_str::<Value>(value).is_ok())
            }
            Self::DataUri => value
                .as_str()
                .is_some_and(|value| value.starts_with("data:") && value.contains(";base64,")),
            Self::Zip => value.as_str().is_some_and(|value| {
                let len = value.chars().count();
                (3..=20).contains(&len)
                    && value.chars().all(|ch| {
                        ch.is_ascii_alphanumeric() || ch == '-' || ch.is_ascii_whitespace()
                    })
            }),
            Self::Icon => value.as_str().is_some_and(|value| !value.trim().is_empty()),
            Self::Color => value.as_str().is_some_and(is_color),
            Self::Guid => value.as_str().is_some_and(is_guid),
            Self::TinyInt => {
                value
                    .as_i64()
                    .is_some_and(|value| (-128..=127).contains(&value))
                    || value
                        .as_str()
                        .and_then(|value| value.parse::<i64>().ok())
                        .is_some_and(|value| (-128..=127).contains(&value))
            }
            Self::SmallInt => {
                value
                    .as_i64()
                    .is_some_and(|value| (-32768..=32767).contains(&value))
                    || value
                        .as_str()
                        .and_then(|value| value.parse::<i64>().ok())
                        .is_some_and(|value| (-32768..=32767).contains(&value))
            }
        }
    }

    fn transform(self, value: &Value) -> Value {
        match self {
            Self::Number => value
                .as_str()
                .and_then(|value| value.parse::<f64>().ok())
                .map_or_else(|| value.clone(), |value| json!(value)),
            Self::Bool => value.as_str().map_or_else(
                || value.clone(),
                |value| {
                    let value = value.to_ascii_lowercase();
                    json!(value == "true" || value == "1")
                },
            ),
            Self::Lower | Self::Lowercase => value
                .as_str()
                .map_or_else(|| value.clone(), |value| json!(value.to_ascii_lowercase())),
            Self::Uppercase => value
                .as_str()
                .map_or_else(|| value.clone(), |value| json!(value.to_ascii_uppercase())),
            Self::Capitalize | Self::Capitalize2 | Self::Name => value
                .as_str()
                .map_or_else(|| value.clone(), |value| json!(capitalize_words(value))),
            Self::Json => value.as_str().map_or_else(
                || value.clone(),
                |value| {
                    serde_json::from_str::<Value>(value)
                        .unwrap_or_else(|_| Value::String(value.to_string()))
                },
            ),
            Self::TinyInt | Self::SmallInt => value
                .as_str()
                .and_then(|value| value.parse::<i64>().ok())
                .map_or_else(|| value.clone(), |value| json!(value)),
            _ => value.clone(),
        }
    }

    fn code(self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::String => "string",
            Self::Number => "number",
            Self::Bool => "boolean",
            Self::Array => "array",
            Self::Object => "object",
            Self::Email => "email",
            Self::Uid => "uid",
            Self::Date => "date",
            Self::Phone => "phone",
            Self::Lower => "lower",
            Self::Lowercase => "lowercase",
            Self::Uppercase => "uppercase",
            Self::Capitalize => "capitalize",
            Self::Capitalize2 => "capitalize2",
            Self::Name => "name",
            Self::Base64 => "base64",
            Self::Url => "url",
            Self::Json => "json",
            Self::DataUri => "datauri",
            Self::Zip => "zip",
            Self::Icon => "icon",
            Self::Color => "color",
            Self::Guid => "guid",
            Self::TinyInt => "tinyint",
            Self::SmallInt => "smallint",
        }
    }

    fn message(self) -> &'static str {
        match self {
            Self::Any => "invalid value",
            Self::String => "expected string",
            Self::Number => "expected number",
            Self::Bool => "expected boolean",
            Self::Array => "expected array",
            Self::Object => "expected object",
            Self::Email => "expected email",
            Self::Uid => "expected uid",
            Self::Date => "expected date",
            Self::Phone => "expected phone",
            Self::Lower => "expected lowercase string",
            Self::Lowercase => "expected lowercase string",
            Self::Uppercase => "expected uppercase string",
            Self::Capitalize => "expected capitalized string",
            Self::Capitalize2 => "expected capitalized string",
            Self::Name => "expected name string",
            Self::Base64 => "expected base64 string",
            Self::Url => "expected URL",
            Self::Json => "expected JSON",
            Self::DataUri => "expected data URI",
            Self::Zip => "expected ZIP/postal code",
            Self::Icon => "expected icon",
            Self::Color => "expected color",
            Self::Guid => "expected GUID",
            Self::TinyInt => "expected tiny integer",
            Self::SmallInt => "expected small integer",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CompatRoute {
    method: RouteMethod,
    path: String,
    action: String,
    api: Option<CompatApiEndpoint>,
    auth: RouteAuth,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CompatApiEndpoint {
    name: String,
    params: Vec<(usize, String)>,
}

impl CompatRoute {
    fn parse(expression: &str) -> Result<Self, Error> {
        let Some((left, action)) = expression.split_once("-->") else {
            return Err(Error::bad_request(format!(
                "compat route must contain '-->': {expression}"
            )));
        };
        let action = normalize_action_target(action)?;
        let mut tokens = left.split_whitespace();
        let first = tokens
            .next()
            .ok_or_else(|| Error::bad_request("missing route method"))?;
        let auth = route_auth(first);
        let first_clean = first.trim_start_matches(['+', '-', '#', '%']);
        if first_clean.eq_ignore_ascii_case("API") {
            let base = tokens.next().unwrap_or("/");
            let api_token = tokens.next().ok_or_else(|| {
                Error::bad_request(format!("missing API operation: {expression}"))
            })?;
            let endpoint = api_token.trim_start_matches(['+', '-', '#', '%']);
            let mut segments = endpoint.split('/');
            let name = segments.next().unwrap_or_default().to_string();
            if name.is_empty() {
                return Err(Error::bad_request(format!(
                    "missing API operation: {expression}"
                )));
            }
            let params = segments
                .enumerate()
                .filter_map(|(index, segment)| {
                    let name = segment.trim_matches(['{', '}']);
                    (!name.is_empty()).then(|| (index + 1, name.to_string()))
                })
                .collect();
            Ok(Self {
                method: RouteMethod::Post,
                path: base.to_string(),
                action,
                api: Some(CompatApiEndpoint { name, params }),
                auth,
            })
        } else {
            let method = RouteMethod::parse(first_clean).ok_or_else(|| {
                Error::bad_request(format!("invalid compat route method: {expression}"))
            })?;
            let path = tokens
                .next()
                .ok_or_else(|| Error::bad_request(format!("missing route path: {expression}")))?;
            Ok(Self {
                method,
                path: normalize_route_path(path)?,
                action,
                api: None,
                auth,
            })
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationError {
    pub field: String,
    pub error: String,
    pub message: String,
}

impl ValidationError {
    pub fn new(
        field: impl Into<String>,
        error: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            field: field.into(),
            error: error.into(),
            message: message.into(),
        }
    }
}

/// Route registration helper for plugin/controller prefixes.
pub struct RouteGroup<'a> {
    app: &'a mut Total,
    prefix: String,
}

impl RouteGroup<'_> {
    pub fn route<F, Fut>(&mut self, expression: &str, handler: F) -> Result<&mut Self, Error>
    where
        F: Fn(Context) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Response> + Send + 'static,
    {
        let (method, path, auth) = parse_route_expression(expression)?;
        let path = join_route_paths(&self.prefix, &path)?;
        self.app.add_route_with_auth(method, path, auth, handler)?;
        Ok(self)
    }

    pub fn route_compat(&mut self, expression: &str) -> Result<&mut Self, Error> {
        let route = CompatRoute::parse(expression)?;
        let path = join_route_paths(&self.prefix, &route.path)?;
        let action = route.action.clone();
        self.app
            .add_route_with_auth(route.method, path, route.auth, move |ctx| {
                let action = action.clone();
                async move { ctx.action_success(&action, ctx.action_input()).await }
            })?;
        Ok(self)
    }

    pub fn add_route<F, Fut>(
        &mut self,
        method: RouteMethod,
        path: impl Into<String>,
        handler: F,
    ) -> Result<&mut Self, Error>
    where
        F: Fn(Context) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Response> + Send + 'static,
    {
        let path = join_route_paths(&self.prefix, &path.into())?;
        self.app.add_route(method, path, handler)?;
        Ok(self)
    }
}

/// Request context passed to handlers and actions.
#[derive(Debug, Clone)]
pub struct UploadedFile {
    pub field: String,
    pub filename: String,
    pub content_type: String,
    pub data: Bytes,
}

#[derive(Clone)]
pub struct Context {
    state: Arc<AppState>,
    pub method: Method,
    pub uri: Uri,
    pub params: HashMap<String, String>,
    pub query: HashMap<String, String>,
    pub headers: HeaderMap,
    pub body: Bytes,
    pub fields: HashMap<String, String>,
    pub files: Vec<UploadedFile>,
    data: HashMap<String, Value>,
    user: Option<Value>,
    model: Value,
}

impl Context {
    pub fn config(&self) -> &Config {
        &self.state.config
    }

    pub fn conf(&self) -> &Config {
        self.config()
    }

    pub fn paths(&self) -> &TPath {
        &self.state.paths
    }

    pub fn plugins(&self) -> &HashMap<String, Plugin> {
        &self.state.plugins
    }

    pub fn flowstreams(&self) -> &HashMap<String, FlowStream> {
        &self.state.flowstreams
    }

    pub fn data(&self) -> &Data {
        &self.state.data
    }

    pub fn db(&self) -> Db {
        Db::new()
    }

    pub fn filestorage(&self) -> &FileStorage {
        &self.state.filestorage
    }

    pub fn url(&self) -> &str {
        self.uri.path()
    }

    pub fn user_agent(&self) -> Option<&str> {
        self.headers
            .get(header::USER_AGENT)
            .and_then(|value| value.to_str().ok())
    }

    pub fn ua(&self) -> Option<&str> {
        self.user_agent()
    }

    pub fn cookie(&self, name: &str) -> Option<String> {
        self.headers
            .get(header::COOKIE)
            .and_then(|value| value.to_str().ok())
            .and_then(|cookies| {
                cookies.split(';').find_map(|cookie| {
                    let (key, value) = cookie.trim().split_once('=')?;
                    (key == name).then(|| percent_decode(value))
                })
            })
    }

    pub fn signed_cookie(&self, name: &str) -> Option<String> {
        let signed = self.cookie(name)?;
        let (value, signature) = signed.rsplit_once('.')?;
        let secret = self.config().string("secret").unwrap_or_default();
        verify_cookie_signature(value, signature, secret).then(|| value.to_string())
    }

    pub fn ip(&self) -> Option<&str> {
        self.headers
            .get("x-forwarded-for")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(',').next())
            .map(str::trim)
            .or_else(|| {
                self.headers
                    .get("x-real-ip")
                    .and_then(|value| value.to_str().ok())
            })
    }

    pub fn user(&self) -> Option<&Value> {
        self.user.as_ref()
    }

    pub fn set_user(&mut self, user: impl Serialize) -> Result<(), Error> {
        self.user = Some(serde_json::to_value(user).map_err(|err| {
            Error::internal(format!("failed to serialize authenticated user: {err}"))
        })?);
        Ok(())
    }

    pub fn clear_user(&mut self) {
        self.user = None;
    }

    pub fn is_authenticated(&self) -> bool {
        self.user.is_some()
    }

    pub fn model(&self) -> &Value {
        &self.model
    }

    pub fn value(&self) -> &Value {
        self.model()
    }

    pub fn param(&self, name: &str) -> Option<&str> {
        self.params.get(name).map(String::as_str)
    }

    pub fn params(&self) -> &HashMap<String, String> {
        &self.params
    }

    pub fn param_required(&self, name: &str) -> Result<&str, Error> {
        self.param(name)
            .ok_or_else(|| Error::bad_request(format!("missing route parameter: {name}")))
    }

    pub fn query(&self, name: &str) -> Option<&str> {
        self.query.get(name).map(String::as_str)
    }

    pub fn queries(&self) -> &HashMap<String, String> {
        &self.query
    }

    pub fn query_required(&self, name: &str) -> Result<&str, Error> {
        self.query(name)
            .ok_or_else(|| Error::bad_request(format!("missing query parameter: {name}")))
    }

    pub fn set(&mut self, name: impl Into<String>, value: impl Serialize) -> Result<(), Error> {
        let value = serde_json::to_value(value)
            .map_err(|err| Error::internal(format!("failed to serialize context data: {err}")))?;
        self.data.insert(name.into(), value);
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&Value> {
        self.data.get(name)
    }

    pub fn json_body<T>(&self) -> Result<T, Error>
    where
        T: serde::de::DeserializeOwned,
    {
        serde_json::from_slice(&self.body)
            .map_err(|err| Error::bad_request(format!("invalid JSON body: {err}")))
    }

    pub fn field(&self, name: &str) -> Option<&str> {
        self.fields.get(name).map(String::as_str)
    }

    pub fn files(&self) -> &[UploadedFile] {
        &self.files
    }

    pub fn body<T>(&self) -> Result<T, Error>
    where
        T: serde::de::DeserializeOwned,
    {
        self.json_body()
    }

    pub fn action_input(&self) -> Value {
        let mut output = match serde_json::from_slice::<Value>(&self.body) {
            Ok(Value::Object(map)) => map,
            _ => Map::new(),
        };
        for (key, value) in &self.params {
            output.entry(key.clone()).or_insert_with(|| json!(value));
        }
        for (key, value) in &self.query {
            output.entry(key.clone()).or_insert_with(|| json!(value));
        }
        for (key, value) in &self.fields {
            output.entry(key.clone()).or_insert_with(|| json!(value));
        }
        Value::Object(output)
    }

    pub fn callback<T: Serialize>(&self, value: T) -> Response {
        Response::json(value)
    }

    pub fn success<T: Serialize>(&self, value: T) -> Response {
        Response::success(value)
    }

    pub fn invalid(&self, status: StatusCode, message: impl Into<String>) -> Response {
        Response::json(json!({
            "success": false,
            "error": message.into(),
            "status": status.as_u16()
        }))
        .status(status)
    }

    pub fn done<T: Serialize>(&self, value: T) -> Response {
        self.success(value)
    }

    pub async fn action(&self, name: &str, input: Value) -> Result<Value, Error> {
        let action = self
            .state
            .actions
            .get(name)
            .cloned()
            .ok_or_else(|| Error::not_found(format!("action not found: {name}")))?;
        let mut ctx = self.clone();
        ctx.model = input.clone();
        action(ctx, input).await
    }

    pub async fn action_success(&self, name: &str, input: Value) -> Response {
        match self.action(name, input).await {
            Ok(value) => Response::success(value),
            Err(err) => err.into_response(),
        }
    }

    pub async fn flow_input<T: Serialize>(
        &self,
        stream: &str,
        input: &str,
        data: T,
    ) -> Result<Value, Error> {
        let stream_ref = self
            .state
            .flowstreams
            .get(stream)
            .ok_or_else(|| Error::not_found(format!("flowstream not found: {stream}")))?;
        let data = serde_json::to_value(data)
            .map_err(|err| Error::internal(format!("failed to serialize flow input: {err}")))?;
        stream_ref.input_exec(input, data).await
    }

    pub async fn flow_rpc<T: Serialize>(
        &self,
        stream: &str,
        rpc: &str,
        data: T,
    ) -> Result<Value, Error> {
        let stream_ref = self
            .state
            .flowstreams
            .get(stream)
            .ok_or_else(|| Error::not_found(format!("flowstream not found: {stream}")))?;
        let data = serde_json::to_value(data)
            .map_err(|err| Error::internal(format!("failed to serialize flow rpc: {err}")))?;
        stream_ref.rpc_exec(rpc, data).await
    }
}

pub type Dollar = Context;

pub struct WsContext {
    state: Arc<AppState>,
    incoming: tokio::sync::mpsc::Receiver<Result<Vec<u8>, String>>,
    outgoing: tokio::sync::mpsc::Sender<Vec<u8>>,
    pub uri: Uri,
    pub headers: HeaderMap,
    pub params: HashMap<String, String>,
    pub query: HashMap<String, String>,
    user: Option<Value>,
    protocol: Option<String>,
    connection_id: String,
}

impl WsContext {
    pub fn config(&self) -> &Config {
        &self.state.config
    }

    pub fn param(&self, name: &str) -> Option<&str> {
        self.params.get(name).map(String::as_str)
    }

    pub fn query(&self, name: &str) -> Option<&str> {
        self.query.get(name).map(String::as_str)
    }

    pub fn user(&self) -> Option<&Value> {
        self.user.as_ref()
    }

    pub fn protocol(&self) -> Option<&str> {
        self.protocol.as_deref()
    }

    pub fn connection_id(&self) -> &str {
        &self.connection_id
    }

    pub fn connection_count(&self) -> usize {
        self.state
            .ws_connections
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }

    pub async fn broadcast_text(&self, value: impl Into<String>) -> usize {
        let value = value.into().into_bytes();
        let connections = self
            .state
            .ws_connections
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let mut sent = 0;
        for connection in connections {
            if connection.send(value.clone()).await.is_ok() {
                sent += 1;
            }
        }
        sent
    }

    pub async fn send_text(&mut self, value: impl Into<String>) -> Result<(), Error> {
        self.outgoing
            .send(value.into().into_bytes())
            .await
            .map_err(|_| Error::internal("websocket connection is closed"))
    }

    pub async fn send_json<T: Serialize>(&mut self, value: T) -> Result<(), Error> {
        let value = serde_json::to_string(&value)
            .map_err(|err| Error::internal(format!("websocket JSON failed: {err}")))?;
        self.send_text(value).await
    }

    pub async fn recv(&mut self) -> Option<Result<String, Error>> {
        self.incoming.recv().await.map(|result| {
            result
                .map(|value| String::from_utf8_lossy(&value).into_owned())
                .map_err(Error::internal)
        })
    }
}

#[derive(Debug, Clone)]
pub struct CookieOptions {
    pub path: String,
    pub domain: Option<String>,
    pub max_age: Option<i64>,
    pub expires: Option<DateTime<Utc>>,
    pub http_only: bool,
    pub secure: bool,
    pub same_site: Option<String>,
    pub secret: Option<String>,
}

impl Default for CookieOptions {
    fn default() -> Self {
        Self {
            path: "/".to_string(),
            domain: None,
            max_age: None,
            expires: None,
            http_only: true,
            secure: false,
            same_site: Some("Lax".to_string()),
            secret: None,
        }
    }
}

/// Framework response wrapper.
#[derive(Debug, Clone)]
pub struct Response {
    status: StatusCode,
    body: ResponseBody,
    headers: HeaderMap,
}

#[derive(Debug, Clone)]
enum ResponseBody {
    Empty,
    Text(String),
    Json(Value),
    Bytes(Vec<u8>, &'static str),
}

impl Response {
    pub fn empty() -> Self {
        Self {
            status: StatusCode::NO_CONTENT,
            body: ResponseBody::Empty,
            headers: HeaderMap::new(),
        }
    }

    pub fn text(value: impl Into<String>) -> Self {
        Self {
            status: StatusCode::OK,
            body: ResponseBody::Text(value.into()),
            headers: HeaderMap::new(),
        }
    }

    pub fn json<T: Serialize>(value: T) -> Self {
        let body = serde_json::to_value(value)
            .unwrap_or_else(|_| json!({ "error": "response serialization failed" }));
        Self {
            status: StatusCode::OK,
            body: ResponseBody::Json(body),
            headers: HeaderMap::new(),
        }
    }

    pub fn success<T: Serialize>(value: T) -> Self {
        Self::json(json!({ "success": true, "value": value }))
    }

    pub fn bytes(bytes: impl Into<Vec<u8>>, content_type: &'static str) -> Self {
        Self {
            status: StatusCode::OK,
            body: ResponseBody::Bytes(bytes.into(), content_type),
            headers: HeaderMap::new(),
        }
    }

    pub fn status(mut self, status: StatusCode) -> Self {
        self.status = status;
        self
    }

    pub fn header(mut self, name: header::HeaderName, value: header::HeaderValue) -> Self {
        self.headers.insert(name, value);
        self
    }

    pub fn cookie(
        mut self,
        name: &str,
        value: &str,
        options: CookieOptions,
    ) -> Result<Self, Error> {
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        {
            return Err(Error::bad_request("invalid cookie name"));
        }
        let value = if let Some(secret) = options.secret.as_deref() {
            format!("{value}.{}", cookie_signature(value, secret))
        } else {
            value.to_string()
        };
        let mut cookie = format!("{name}={}", percent_encode_cookie(&value));
        if !options.path.is_empty() {
            cookie.push_str(&format!("; Path={}", options.path));
        }
        if let Some(domain) = options.domain {
            cookie.push_str(&format!("; Domain={domain}"));
        }
        if let Some(max_age) = options.max_age {
            cookie.push_str(&format!("; Max-Age={max_age}"));
        }
        if let Some(expires) = options.expires {
            cookie.push_str(&format!(
                "; Expires={}",
                expires.format("%a, %d %b %Y %H:%M:%S GMT")
            ));
        }
        if options.http_only {
            cookie.push_str("; HttpOnly");
        }
        if options.secure {
            cookie.push_str("; Secure");
        }
        if let Some(same_site) = options.same_site {
            if matches!(
                same_site.to_ascii_lowercase().as_str(),
                "strict" | "lax" | "none"
            ) {
                cookie.push_str(&format!("; SameSite={same_site}"));
            }
        }
        let value = header::HeaderValue::from_str(&cookie)
            .map_err(|_| Error::bad_request("invalid cookie value"))?;
        self.headers.append(header::SET_COOKIE, value);
        Ok(self)
    }

    fn into_http_response(self) -> http::Response<Bytes> {
        let mut builder = http::Response::builder().status(self.status);
        if let Some(headers) = builder.headers_mut() {
            headers.extend(self.headers);
        }
        let body = match self.body {
            ResponseBody::Empty => Bytes::new(),
            ResponseBody::Text(value) => {
                builder = builder.header(header::CONTENT_TYPE, "text/plain; charset=utf-8");
                Bytes::from(value)
            }
            ResponseBody::Json(value) => {
                builder = builder.header(header::CONTENT_TYPE, "application/json");
                Bytes::from(serde_json::to_vec(&value).unwrap_or_default())
            }
            ResponseBody::Bytes(value, content_type) => {
                builder = builder.header(header::CONTENT_TYPE, content_type);
                Bytes::from(value)
            }
        };
        builder.body(body).unwrap_or_else(|_| {
            http::Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Bytes::from_static(b"response serialization failed"))
                .expect("static response is valid")
        })
    }
}

/// Public framework error with safe JSON output.
#[derive(Debug, Clone)]
pub struct Error {
    pub status: StatusCode,
    pub message: String,
    pub validation: Vec<ValidationError>,
}

impl Error {
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
            validation: Vec::new(),
        }
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
            validation: Vec::new(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
            validation: Vec::new(),
        }
    }

    pub fn validation(errors: Vec<ValidationError>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: "validation failed".to_string(),
            validation: errors,
        }
    }

    pub fn into_response(self) -> Response {
        if self.validation.is_empty() {
            Response::json(json!({
                "success": false,
                "error": self.message,
                "status": self.status.as_u16()
            }))
            .status(self.status)
        } else {
            Response::json(json!({
                "success": false,
                "error": self.message,
                "status": self.status.as_u16(),
                "errors": self.validation
            }))
            .status(self.status)
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.status, self.message)
    }
}

impl std::error::Error for Error {}

impl From<Error> for Response {
    fn from(value: Error) -> Self {
        value.into_response()
    }
}

async fn dispatch_native(
    state: Arc<AppState>,
    params: HashMap<String, String>,
    query: HashMap<String, String>,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
    handler: Handler,
    method: Method,
    auth_required: RouteAuth,
) -> Response {
    if body.len() > state.config.body_limit {
        return Error::bad_request("request body is too large").into_response();
    }

    let multipart = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .filter(|value| {
            value
                .to_ascii_lowercase()
                .starts_with("multipart/form-data")
        })
        .map(|content_type| parse_multipart(&body, content_type, &state.config));
    let (fields, files) = match multipart {
        Some(Ok(value)) => value,
        Some(Err(err)) => return err.into_response(),
        None => (HashMap::new(), Vec::new()),
    };

    let ctx = Context {
        state,
        method,
        uri,
        params,
        query,
        headers,
        body,
        fields,
        files,
        data: HashMap::new(),
        user: None,
        model: Value::Null,
    };

    let middlewares = ctx.state.middlewares.clone();
    let mut ctx = ctx;
    if let Some(auth) = ctx.state.auth.clone() {
        match auth(ctx).await {
            Ok(next) => ctx = next,
            Err(response) => return response,
        }
    }
    let authorized = match auth_required {
        RouteAuth::Any => true,
        RouteAuth::Member => ctx.is_authenticated(),
        RouteAuth::Guest => !ctx.is_authenticated(),
    };
    if !authorized {
        return Error {
            status: StatusCode::UNAUTHORIZED,
            message: "Unauthorized".to_string(),
            validation: Vec::new(),
        }
        .into_response();
    }
    for middleware in middlewares.iter() {
        match middleware(ctx).await {
            Ok(next) => ctx = next,
            Err(response) => return response,
        }
    }

    handler(ctx).await
}

async fn dispatch_api_native(
    state: Arc<AppState>,
    query: HashMap<String, String>,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
    endpoints: Arc<HashMap<String, ApiEndpointDef>>,
) -> Response {
    dispatch_native(
        state,
        HashMap::new(),
        query,
        uri,
        headers,
        body,
        api_handler(endpoints),
        Method::POST,
        RouteAuth::Any,
    )
    .await
}

fn api_handler(endpoints: Arc<HashMap<String, ApiEndpointDef>>) -> Handler {
    Arc::new(move |mut ctx| {
        let endpoints = endpoints.clone();
        Box::pin(async move {
            let envelope: Value = match serde_json::from_slice(&ctx.body) {
                Ok(value) => value,
                Err(_) => return Error::bad_request("Invalid data").into_response(),
            };
            let Some(schema) = envelope.get("schema").and_then(Value::as_str) else {
                return Error::bad_request("Invalid data").into_response();
            };
            let (schema, query) = schema.split_once('?').unwrap_or((schema, ""));
            let mut segments = schema.split('/');
            let endpoint_name = segments.next().unwrap_or_default();
            let Some(endpoint) = endpoints.get(endpoint_name) else {
                return Error::bad_request("Invalid data").into_response();
            };
            let authorized = match endpoint.auth {
                RouteAuth::Any => true,
                RouteAuth::Member => ctx.is_authenticated(),
                RouteAuth::Guest => !ctx.is_authenticated(),
            };
            if !authorized {
                return Error {
                    status: StatusCode::UNAUTHORIZED,
                    message: "Unauthorized".to_string(),
                    validation: Vec::new(),
                }
                .into_response();
            }
            let segments = segments.collect::<Vec<_>>();
            for (index, name) in &endpoint.params {
                let value = segments.get(index - 1).copied().unwrap_or_default();
                if value.is_empty() {
                    return Error::bad_request(format!("missing API parameter: {name}"))
                        .into_response();
                }
                ctx.params.insert(name.clone(), value.to_string());
            }
            ctx.query = parse_urlencoded(query);
            let data = envelope.get("data").cloned().unwrap_or(Value::Null);
            if !data.is_null() && !data.is_object() {
                return Error::bad_request("Invalid data").into_response();
            }
            ctx.body = if data.is_null() {
                Bytes::new()
            } else {
                Bytes::from(serde_json::to_vec(&data).unwrap_or_default())
            };
            let input = ctx.action_input();
            ctx.action_success(&endpoint.action, input).await
        })
    })
}

fn parse_urlencoded(value: &str) -> HashMap<String, String> {
    value
        .split('&')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let (key, value) = part.split_once('=').unwrap_or((part, ""));
            (percent_decode(key), percent_decode(value))
        })
        .collect()
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => output.push(b' '),
            b'%' if index + 2 < bytes.len() => {
                let hex = &value[index + 1..index + 3];
                if let Ok(byte) = u8::from_str_radix(hex, 16) {
                    output.push(byte);
                    index += 2;
                } else {
                    output.push(bytes[index]);
                }
            }
            byte => output.push(byte),
        }
        index += 1;
    }
    String::from_utf8_lossy(&output).into_owned()
}

fn percent_encode_cookie(value: &str) -> String {
    let mut output = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            output.push(byte as char);
        } else {
            output.push_str(&format!("%{byte:02X}"));
        }
    }
    output
}

fn cookie_signature(value: &str, secret: &str) -> String {
    let mut mac = Hmac::<sha2::Sha256>::new_from_slice(secret.as_bytes())
        .expect("HMAC accepts keys of every length");
    mac.update(value.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
}

fn verify_cookie_signature(value: &str, signature: &str, secret: &str) -> bool {
    let Ok(signature) = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(signature) else {
        return false;
    };
    let mut mac = Hmac::<sha2::Sha256>::new_from_slice(secret.as_bytes())
        .expect("HMAC accepts keys of every length");
    mac.update(value.as_bytes());
    mac.verify_slice(&signature).is_ok()
}

fn parse_multipart(
    body: &[u8],
    content_type: &str,
    config: &Config,
) -> Result<(HashMap<String, String>, Vec<UploadedFile>), Error> {
    let boundary = content_type
        .split(';')
        .map(str::trim)
        .find_map(|part| part.strip_prefix("boundary="))
        .map(|value| value.trim_matches('"'))
        .filter(|value| !value.is_empty() && value.len() <= 200)
        .ok_or_else(|| Error::bad_request("multipart boundary is missing or invalid"))?;
    let delimiter = format!("--{boundary}").into_bytes();
    let max_fields = config
        .get("$httpmaxkeys")
        .and_then(Value::as_u64)
        .unwrap_or(33) as usize;
    let max_name = config
        .get("$httpmaxkey")
        .and_then(Value::as_u64)
        .unwrap_or(25) as usize;
    let mut fields = HashMap::new();
    let mut files = Vec::new();
    let mut cursor = 0;
    while let Some(start) = find_slice(&body[cursor..], &delimiter) {
        cursor += start + delimiter.len();
        if body.get(cursor..cursor + 2) == Some(b"--") {
            break;
        }
        if body.get(cursor..cursor + 2) != Some(b"\r\n") {
            return Err(Error::bad_request("malformed multipart delimiter"));
        }
        cursor += 2;
        let header_end = find_slice(&body[cursor..], b"\r\n\r\n")
            .ok_or_else(|| Error::bad_request("incomplete multipart headers"))?;
        let header_bytes = &body[cursor..cursor + header_end];
        let header_text = std::str::from_utf8(header_bytes)
            .map_err(|_| Error::bad_request("multipart headers are not UTF-8"))?;
        cursor += header_end + 4;
        let next_marker = [b"\r\n".as_slice(), delimiter.as_slice()].concat();
        let data_end = find_slice(&body[cursor..], &next_marker)
            .ok_or_else(|| Error::bad_request("incomplete multipart field"))?;
        let data = &body[cursor..cursor + data_end];
        cursor += data_end + 2;

        let mut disposition = None;
        let mut part_content_type = "application/octet-stream".to_string();
        for line in header_text.split("\r\n") {
            let Some((name, value)) = line.split_once(':') else {
                return Err(Error::bad_request("malformed multipart header"));
            };
            if name.trim().eq_ignore_ascii_case("content-disposition") {
                disposition = Some(value.trim());
            } else if name.trim().eq_ignore_ascii_case("content-type") {
                part_content_type = value.trim().to_string();
            }
        }
        let disposition = disposition
            .filter(|value| value.to_ascii_lowercase().starts_with("form-data"))
            .ok_or_else(|| Error::bad_request("multipart content-disposition is missing"))?;
        let name = disposition_parameter(disposition, "name")
            .filter(|name| !name.is_empty() && name.len() <= max_name)
            .ok_or_else(|| Error::bad_request("multipart field name is missing or too long"))?;
        if fields.len() + files.len() >= max_fields {
            return Err(Error::bad_request("too many multipart fields"));
        }
        if let Some(filename) = disposition_parameter(disposition, "filename") {
            let filename = filename
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or_default()
                .replace('\0', "");
            if filename.is_empty() {
                return Err(Error::bad_request("multipart filename is invalid"));
            }
            files.push(UploadedFile {
                field: name.to_string(),
                filename,
                content_type: part_content_type,
                data: Bytes::copy_from_slice(data),
            });
        } else {
            let value = std::str::from_utf8(data)
                .map_err(|_| Error::bad_request("multipart text field is not UTF-8"))?;
            fields.insert(name.to_string(), value.to_string());
        }
    }
    Ok((fields, files))
}

fn disposition_parameter<'a>(value: &'a str, expected: &str) -> Option<&'a str> {
    value.split(';').skip(1).map(str::trim).find_map(|part| {
        let (name, value) = part.split_once('=')?;
        name.trim()
            .eq_ignore_ascii_case(expected)
            .then(|| value.trim().trim_matches('"'))
    })
}

fn find_slice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    (!needle.is_empty())
        .then(|| {
            haystack
                .windows(needle.len())
                .position(|window| window == needle)
        })
        .flatten()
}

async fn static_file_response(state: &AppState, uri_path: &str) -> Result<Option<Response>, Error> {
    let relative = uri_path.trim_start_matches('/');
    if relative.is_empty() {
        return Ok(None);
    }

    let public_root = state.paths.public(None);
    let file = safe_join(&public_root, relative)?;

    match fs::metadata(&file).await {
        Ok(metadata) if metadata.is_file() => {
            let bytes = fs::read(&file)
                .await
                .map_err(|err| Error::internal(format!("failed to read static file: {err}")))?;
            let content_type = content_type_for(&file);
            Ok(Some(
                Response::bytes(bytes, content_type)
                    .header(
                        header::ACCEPT_RANGES,
                        header::HeaderValue::from_static("bytes"),
                    )
                    .header(
                        header::CACHE_CONTROL,
                        header::HeaderValue::from_str(&format!(
                            "public, max-age={}",
                            state.config.public_max_age
                        ))
                        .unwrap_or_else(|_| header::HeaderValue::from_static("public")),
                    ),
            ))
        }
        _ => Ok(None),
    }
}

fn parse_route_expression(expression: &str) -> Result<(RouteMethod, String, RouteAuth), Error> {
    let mut parts = expression.split_whitespace();
    let method_token = parts
        .next()
        .ok_or_else(|| Error::bad_request(format!("invalid route expression: {expression}")))?;
    let auth = route_auth(method_token);
    let method = RouteMethod::parse(method_token.trim_start_matches(['+', '-']))
        .ok_or_else(|| Error::bad_request(format!("invalid route expression: {expression}")))?;
    let path = parts
        .next()
        .ok_or_else(|| Error::bad_request(format!("missing route path: {expression}")))?;
    Ok((method, path.to_string(), auth))
}

fn route_auth(method: &str) -> RouteAuth {
    match method.as_bytes().first() {
        Some(b'+') => RouteAuth::Member,
        Some(b'-') => RouteAuth::Guest,
        _ => RouteAuth::Any,
    }
}

fn normalize_route_path(path: &str) -> Result<String, Error> {
    let trimmed = path.trim();
    if !trimmed.starts_with('/') {
        return Err(Error::bad_request(format!(
            "route path must start with '/': {trimmed}"
        )));
    }
    Ok(trimmed.to_string())
}

fn normalize_schema_name(name: &str) -> Result<String, Error> {
    let name = name.trim().trim_matches('/');
    if name.is_empty() {
        return Err(Error::bad_request("schema name cannot be empty"));
    }
    Ok(name.to_string())
}

fn normalize_action_name(name: &str) -> String {
    name.trim().trim_matches('/').to_string()
}

fn normalize_action_target(value: &str) -> Result<String, Error> {
    let target = value
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .trim()
        .trim_start_matches(['+', '-', '#', '%', '@'])
        .trim_matches('/');
    if target.is_empty() {
        Err(Error::bad_request("missing action target"))
    } else {
        Ok(target.replace('|', "/"))
    }
}

fn normalize_route_prefix(prefix: &str) -> Result<String, Error> {
    let prefix = normalize_route_path(prefix)?;
    if prefix == "/" {
        Ok(prefix)
    } else {
        Ok(prefix.trim_end_matches('/').to_string())
    }
}

fn resolve_api_base(base: &str, default: &str) -> String {
    if !base.contains('?') {
        return base.to_string();
    }
    let joined = base.replace('?', default);
    let mut output = String::with_capacity(joined.len());
    for ch in joined.chars() {
        if ch != '/' || !output.ends_with('/') {
            output.push(ch);
        }
    }
    output
}

fn join_route_paths(prefix: &str, path: &str) -> Result<String, Error> {
    let path = normalize_route_path(path)?;
    if prefix == "/" || prefix.is_empty() {
        return Ok(path);
    }
    if path == "/" {
        return Ok(format!("{prefix}/"));
    }
    Ok(format!(
        "{}/{}",
        prefix.trim_end_matches('/'),
        path.trim_start_matches('/')
    ))
}

fn safe_join(root: &Path, relative: &str) -> Result<PathBuf, Error> {
    let mut path = root.to_path_buf();
    for segment in relative.split('/') {
        if segment.is_empty() || segment == "." {
            continue;
        }
        if segment == ".." || segment.contains('\\') {
            return Err(Error::not_found("static file not found"));
        }
        path.push(segment);
    }
    Ok(path)
}

fn content_type_for(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
    {
        "css" => "text/css; charset=utf-8",
        "gif" => "image/gif",
        "html" | "htm" => "text/html; charset=utf-8",
        "ico" => "image/x-icon",
        "jpg" | "jpeg" => "image/jpeg",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "png" => "image/png",
        "svg" => "image/svg+xml",
        "txt" | "log" => "text/plain; charset=utf-8",
        "webp" => "image/webp",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}

fn value_length(value: &Value) -> Option<usize> {
    match value {
        Value::String(value) => Some(value.chars().count()),
        Value::Array(value) => Some(value.len()),
        Value::Object(value) => Some(value.len()),
        _ => None,
    }
}

fn is_email(value: &str) -> bool {
    let Some((local, domain)) = value.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && domain.split('.').all(|part| !part.is_empty())
}

fn is_url(value: &str) -> bool {
    let Some((scheme, rest)) = value.split_once("://") else {
        return false;
    };
    matches!(scheme, "http" | "https") && !rest.trim().is_empty() && !rest.contains([' ', '{', '}'])
}

fn is_base64(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && value.len() % 4 == 0
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '+' || ch == '/' || ch == '=')
}

fn is_color(value: &str) -> bool {
    let value = value.trim();
    if let Some(hex) = value.strip_prefix('#') {
        return matches!(hex.len(), 3 | 6 | 8) && hex.chars().all(|ch| ch.is_ascii_hexdigit());
    }
    !value.is_empty()
}

fn is_guid(value: &str) -> bool {
    let parts: Vec<_> = value.split('-').collect();
    let lengths = [8, 4, 4, 4, 12];
    parts.len() == 5
        && parts
            .iter()
            .zip(lengths)
            .all(|(part, len)| part.len() == len && part.chars().all(|ch| ch.is_ascii_hexdigit()))
}

fn capitalize_words(value: &str) -> String {
    value
        .split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str().to_lowercase()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn map_to_value(map: &HashMap<String, String>) -> Value {
    Value::Object(
        map.iter()
            .map(|(key, value)| (key.clone(), Value::String(value.clone())))
            .collect(),
    )
}

fn strip_config_comment(line: &str) -> String {
    let mut quoted = None;
    let mut escaped = false;
    let chars: Vec<char> = line.chars().collect();
    for (index, ch) in chars.iter().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        if *ch == '\\' {
            escaped = true;
            continue;
        }
        match quoted {
            Some(quote) if *ch == quote => quoted = None,
            Some(_) => {}
            None if *ch == '"' || *ch == '\'' => quoted = Some(*ch),
            None if *ch == '#' && comment_marker_allowed(&chars, index) => {
                return chars[..index].iter().collect();
            }
            None if *ch == '/'
                && chars.get(index + 1) == Some(&'/')
                && comment_marker_allowed(&chars, index) =>
            {
                return chars[..index].iter().collect();
            }
            None => {}
        }
    }
    line.to_string()
}

fn comment_marker_allowed(chars: &[char], index: usize) -> bool {
    index == 0
        || chars
            .get(index.wrapping_sub(1))
            .is_some_and(|ch| ch.is_whitespace())
}

fn parse_config_name(raw: &str) -> (String, Option<String>) {
    let raw = raw.trim();
    if let Some(start) = raw.rfind('(') {
        if raw.ends_with(')') && start > 0 {
            let name = raw[..start].trim();
            let type_hint = raw[start + 1..raw.len() - 1].trim();
            if !name.is_empty() && !type_hint.is_empty() {
                return (name.to_string(), Some(type_hint.to_ascii_lowercase()));
            }
        }
    }
    (raw.to_string(), None)
}

fn unquote_config_value(raw: &str) -> Option<String> {
    let raw = raw.trim();
    let mut chars = raw.chars();
    let first = chars.next()?;
    let last = raw.chars().last()?;
    if raw.len() >= 2 && ((first == '"' && last == '"') || (first == '\'' && last == '\'')) {
        Some(
            raw[1..raw.len() - 1]
                .replace("\\\"", "\"")
                .replace("\\'", "'"),
        )
    } else {
        None
    }
}

fn parse_config_value_with_type(
    raw: &str,
    type_hint: Option<&str>,
    current: &HashMap<String, Value>,
) -> Result<(Value, bool), Error> {
    let decoded;
    let raw = if let Some(value) = raw.trim().strip_prefix("base64 ") {
        decoded = String::from_utf8(
            base64::engine::general_purpose::STANDARD
                .decode(value.trim())
                .map_err(|err| Error::internal(format!("invalid base64 config value: {err}")))?,
        )
        .map_err(|err| Error::internal(format!("base64 config value is not UTF-8: {err}")))?;
        decoded.as_str()
    } else if let Some(value) = raw.trim().strip_prefix("hex ") {
        decoded = decode_hex_config(value.trim())?;
        decoded.as_str()
    } else {
        raw.trim()
    };
    if let Some(type_hint) = type_hint {
        match type_hint {
            "string" | "text" => {
                return Ok((
                    Value::String(unquote_config_value(raw).unwrap_or_else(|| raw.to_string())),
                    false,
                ));
            }
            "boolean" | "bool" => {
                return Ok((
                    Value::Bool(matches!(
                        raw.to_ascii_lowercase().as_str(),
                        "true" | "on" | "1" | "enabled"
                    )),
                    false,
                ))
            }
            "number" | "float" | "double" | "currency" => {
                return Ok((
                    raw.parse::<f64>()
                        .map(|value| json!(value))
                        .unwrap_or_else(|_| json!(0)),
                    false,
                ));
            }
            "int" | "integer" => {
                return Ok((
                    raw.parse::<i64>()
                        .map(|value| json!(value))
                        .unwrap_or_else(|_| json!(0)),
                    false,
                ));
            }
            "array" => {
                let value = if raw.starts_with('[') {
                    serde_json::from_str(raw).map_err(|err| {
                        Error::internal(format!("invalid array config value: {err}"))
                    })?
                } else {
                    Value::Array(raw.split(',').map(|item| json!(item.trim())).collect())
                };
                return Ok((value, false));
            }
            "object" | "json" | "eval" => {
                let value = serde_json::from_str(raw).map_err(|err| {
                    Error::internal(format!("invalid {type_hint} config value: {err}"))
                })?;
                return Ok((value, false));
            }
            "env" | "environment" => {
                return Ok((
                    std::env::var(raw).map(Value::String).unwrap_or(Value::Null),
                    false,
                ))
            }
            "config" => return Ok((current.get(raw).cloned().unwrap_or(Value::Null), false)),
            "date" | "time" | "datetime" => {
                let value = chrono::DateTime::parse_from_rfc3339(raw)
                    .map(|date| date.to_rfc3339())
                    .or_else(|_| {
                        chrono::NaiveDate::parse_from_str(raw, "%Y-%m-%d")
                            .map(|date| format!("{date}T00:00:00Z"))
                    })
                    .map_err(|err| Error::internal(format!("invalid date config value: {err}")))?;
                return Ok((Value::String(value), false));
            }
            "random" | "generate" | "hash" => {
                let length = raw.parse::<usize>().unwrap_or(10).max(1);
                let mut value = String::new();
                while value.len() < length {
                    value.push_str(&uuid::Uuid::new_v4().simple().to_string());
                }
                value.truncate(length);
                return Ok((
                    Value::String(value),
                    matches!(type_hint, "generate" | "hash"),
                ));
            }
            _ => {}
        }
    }
    let value = if let Some(value) = unquote_config_value(raw) {
        Value::String(value)
    } else if raw.eq_ignore_ascii_case("true") {
        Value::Bool(true)
    } else if raw.eq_ignore_ascii_case("false") {
        Value::Bool(false)
    } else if let Ok(number) = raw.parse::<i64>() {
        json!(number)
    } else if let Ok(number) = raw.parse::<f64>() {
        json!(number)
    } else if (raw.starts_with('{') && raw.ends_with('}'))
        || (raw.starts_with('[') && raw.ends_with(']'))
    {
        serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.to_string()))
    } else {
        Value::String(raw.to_string())
    };
    Ok((value, false))
}

fn decode_hex_config(raw: &str) -> Result<String, Error> {
    String::from_utf8(decode_hex_bytes(raw)?)
        .map_err(|err| Error::internal(format!("hex config value is not UTF-8: {err}")))
}

fn decode_hex_bytes(raw: &str) -> Result<Vec<u8>, Error> {
    if raw.len() % 2 != 0 {
        return Err(Error::internal("hex config value has an odd length"));
    }
    (0..raw.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&raw[index..index + 2], 16))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| Error::internal(format!("invalid hex config value: {err}")))
}

fn split_schema_fields(fields: &str) -> Vec<String> {
    let mut output = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    for ch in fields.chars() {
        match ch {
            ',' if depth == 0 => {
                output.push(current.trim().to_string());
                current.clear();
            }
            '[' | '(' | '{' => {
                depth += 1;
                current.push(ch);
            }
            ']' | ')' | '}' => {
                depth = depth.saturating_sub(1);
                current.push(ch);
            }
            _ => current.push(ch),
        }
    }
    if !current.trim().is_empty() {
        output.push(current.trim().to_string());
    }
    output
}

fn parse_field_rule(token: &str) -> Result<FieldRule, Error> {
    let mut token = token.trim();
    let required = token.starts_with('*');
    if required {
        token = &token[1..];
    }
    let Some((name, descriptor)) = token.split_once(':') else {
        return Err(Error::bad_request(format!("invalid schema field: {token}")));
    };
    let mut rule = parse_field_descriptor(name.trim(), descriptor)?;
    if required {
        rule = rule.required();
    }
    if let Some(max) = parse_type_limit(descriptor) {
        rule = rule.max(max);
    }
    if let Some(values) = parse_enum_values(descriptor) {
        rule = rule.one_of(values);
    }
    Ok(rule)
}

fn parse_field_descriptor(name: &str, descriptor: &str) -> Result<FieldRule, Error> {
    let descriptor = descriptor.trim();

    if descriptor.starts_with('[') && descriptor.ends_with(']') {
        let inner = descriptor[1..descriptor.len() - 1].trim();
        let mut rule = FieldRule::new(name, FieldKind::Array);
        if !inner.is_empty() {
            if inner.contains(':') {
                rule = rule.array_nested(Validator::parse(inner)?);
            } else {
                rule = rule.array_kind(parse_field_kind(inner));
            }
        }
        return Ok(rule);
    }

    if descriptor.starts_with('{') && descriptor.ends_with('}') {
        let inner = descriptor[1..descriptor.len() - 1].trim();
        if inner.contains('|') && !inner.contains(':') {
            return Ok(FieldRule::new(name, FieldKind::String));
        }
        if inner.contains(':') {
            return Ok(FieldRule::new(name, FieldKind::Object).nested(Validator::parse(inner)?));
        }
    }

    if descriptor.starts_with('@') {
        return Ok(FieldRule::new(name, FieldKind::Object));
    }

    Ok(FieldRule::new(name, parse_field_kind(descriptor)))
}

fn parse_field_kind(descriptor: &str) -> FieldKind {
    let normalized = descriptor
        .trim()
        .trim_start_matches('[')
        .split(['(', '{'])
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    match normalized.as_str() {
        "string" => FieldKind::String,
        "number" => FieldKind::Number,
        "bool" | "boolean" => FieldKind::Bool,
        "any" => FieldKind::Any,
        "array" => FieldKind::Array,
        "object" => FieldKind::Object,
        "email" => FieldKind::Email,
        "uid" => FieldKind::Uid,
        "date" => FieldKind::Date,
        "phone" => FieldKind::Phone,
        "lower" => FieldKind::Lower,
        "lowercase" => FieldKind::Lowercase,
        "uppercase" => FieldKind::Uppercase,
        "name" => FieldKind::Name,
        "base64" => FieldKind::Base64,
        "url" => FieldKind::Url,
        "json" => FieldKind::Json,
        "datauri" => FieldKind::DataUri,
        "zip" => FieldKind::Zip,
        "icon" => FieldKind::Icon,
        "color" => FieldKind::Color,
        "guid" => FieldKind::Guid,
        "tinyint" => FieldKind::TinyInt,
        "smallint" => FieldKind::SmallInt,
        value if value.starts_with("capitalize2") => FieldKind::Capitalize2,
        value if value.starts_with("capitalize") => FieldKind::Capitalize,
        _ if descriptor.trim().starts_with('[') => FieldKind::Array,
        _ if descriptor.contains('{') && descriptor.contains('}') => FieldKind::Object,
        _ => FieldKind::Any,
    }
}

fn parse_type_limit(descriptor: &str) -> Option<usize> {
    if descriptor.trim_start().starts_with('{') {
        return None;
    }
    let start = descriptor.find('(')?;
    let end = descriptor[start + 1..].find(')')? + start + 1;
    descriptor[start + 1..end].trim().parse().ok()
}

fn parse_enum_values(descriptor: &str) -> Option<Vec<String>> {
    let trimmed = descriptor.trim();
    if !(trimmed.contains('{') && trimmed.contains('}')) {
        return None;
    }
    let start = descriptor.find('{')?;
    let end = descriptor[start + 1..].find('}')? + start + 1;
    let inner = descriptor[start + 1..end].trim();
    if inner.contains(':') || !inner.contains('|') {
        return None;
    }
    Some(
        inner
            .split('|')
            .map(|item| item.trim().to_string())
            .filter(|item| !item.is_empty())
            .collect(),
    )
}

pub trait TotalStrExt {
    fn slug(&self) -> String;
    fn parse_json(&self) -> Option<Value>;
    fn is_email(&self) -> bool;
    fn is_url(&self) -> bool;
    fn capitalize_total(&self) -> String;
}

impl TotalStrExt for str {
    fn slug(&self) -> String {
        let mut out = String::new();
        let mut dash = false;
        for ch in self.to_ascii_lowercase().chars() {
            if ch.is_ascii_alphanumeric() {
                out.push(ch);
                dash = false;
            } else if !dash {
                out.push('-');
                dash = true;
            }
        }
        out.trim_matches('-').to_string()
    }

    fn parse_json(&self) -> Option<Value> {
        serde_json::from_str(self).ok()
    }

    fn is_email(&self) -> bool {
        is_email(self)
    }

    fn is_url(&self) -> bool {
        is_url(self)
    }

    fn capitalize_total(&self) -> String {
        capitalize_words(self)
    }
}

impl TotalStrExt for String {
    fn slug(&self) -> String {
        self.as_str().slug()
    }

    fn parse_json(&self) -> Option<Value> {
        self.as_str().parse_json()
    }

    fn is_email(&self) -> bool {
        self.as_str().is_email()
    }

    fn is_url(&self) -> bool {
        self.as_str().is_url()
    }

    fn capitalize_total(&self) -> String {
        self.as_str().capitalize_total()
    }
}

pub trait TotalNumberExt {
    fn floor_dec(self, decimals: u32) -> f64;
    fn format_total(self, decimals: usize) -> String;
    fn vat(self, percentage: f64, included: bool) -> f64;
}

impl TotalNumberExt for f64 {
    fn floor_dec(self, decimals: u32) -> f64 {
        let base = 10_f64.powi(decimals as i32);
        (self * base).floor() / base
    }

    fn format_total(self, decimals: usize) -> String {
        format!("{self:.decimals$}")
    }

    fn vat(self, percentage: f64, included: bool) -> f64 {
        if included {
            self - (self / (1.0 + percentage / 100.0))
        } else {
            self * percentage / 100.0
        }
    }
}

impl TotalNumberExt for i64 {
    fn floor_dec(self, _decimals: u32) -> f64 {
        self as f64
    }

    fn format_total(self, decimals: usize) -> String {
        (self as f64).format_total(decimals)
    }

    fn vat(self, percentage: f64, included: bool) -> f64 {
        (self as f64).vat(percentage, included)
    }
}

pub trait TotalVecExt<T> {
    fn take_total(&self, count: usize) -> Vec<T>
    where
        T: Clone;
    fn first_total(&self) -> Option<&T>;
}

impl<T> TotalVecExt<T> for Vec<T> {
    fn take_total(&self, count: usize) -> Vec<T>
    where
        T: Clone,
    {
        self.iter().take(count).cloned().collect()
    }

    fn first_total(&self) -> Option<&T> {
        self.first()
    }
}

pub trait TotalDateExt {
    fn format_total(&self, format: &str) -> String;
    fn add_days_total(&self, days: i64) -> DateTime<Utc>;
}

impl TotalDateExt for DateTime<Utc> {
    fn format_total(&self, format: &str) -> String {
        self.format(format).to_string()
    }

    fn add_days_total(&self, days: i64) -> DateTime<Utc> {
        *self + chrono::Duration::days(days)
    }
}
/// Register a Total-style route expression on an application.
#[macro_export]
macro_rules! route {
    ($app:expr, $expression:expr) => {
        $app.route_compat($expression)
    };
    ($app:expr, $expression:expr, $handler:path) => {
        $app.route($expression, $handler)
    };
}

/// Declares a convention-loaded Total.rs module.
///
/// The macro intentionally makes application files look like Total.js modules:
/// routes, actions, middleware, configuration, events, and plugin metadata are
/// declared directly in the file, while the macro generates the install hook
/// consumed by build-time discovery.
#[macro_export]
macro_rules! INSTALL {
    ($($body:tt)*) => {
        #[allow(unused_macros)]
        pub fn install(app: &mut ::total5::Total) -> Result<(), ::total5::Error> {
            macro_rules! ROUTE {
                ($expression:expr) => {
                    ::total5::route!(app, $expression)?
                };
                ($expression:expr, $handler:path) => {
                    ::total5::route!(app, $expression, $handler)?
                };
            }

            macro_rules! API {
                ($expression:expr) => {
                    ::total5::route!(app, $expression)?
                };
            }

            macro_rules! NEWACTION {
                ($name:expr, $handler:expr) => {
                    app.action($name, $handler);
                };
                ($name:expr, input: $input:expr, $handler:expr) => {
                    app.action_options($name, ::total5::ActionOptions::new().input($input)?, $handler);
                };
                ($name:expr, params: $params:expr, $handler:expr) => {
                    app.action_options($name, ::total5::ActionOptions::new().params($params)?, $handler);
                };
                ($name:expr, query: $query:expr, $handler:expr) => {
                    app.action_options($name, ::total5::ActionOptions::new().query($query)?, $handler);
                };
            }

            macro_rules! SCHEMA {
                ($name:expr, $schema_body:block) => {
                    app.schema($name, |schema| {
                        macro_rules! ACTION {
                            ($action:expr, $handler:expr) => {
                                schema.action($action, $handler);
                            };
                            ($action:expr, input: $input:expr, $handler:expr) => {
                                schema.action_options($action, ::total5::ActionOptions::new().input($input)?, $handler);
                            };
                            ($action:expr, params: $params:expr, $handler:expr) => {
                                schema.action_options($action, ::total5::ActionOptions::new().params($params)?, $handler);
                            };
                            ($action:expr, query: $query:expr, $handler:expr) => {
                                schema.action_options($action, ::total5::ActionOptions::new().query($query)?, $handler);
                            };
                        }
                        $schema_body
                        Ok(())
                    })?;
                };
            }

            macro_rules! MIDDLEWARE {
                ($handler:expr) => {
                    app.middleware($handler);
                };
            }

            macro_rules! AUTH {
                ($handler:expr) => {
                    app.auth($handler);
                };
            }

            macro_rules! WEBSOCKET {
                ($path:expr, $handler:expr) => {
                    app.websocket($path, $handler)?;
                };
            }

            macro_rules! ON {
                ("load", $handler:expr) => {
                    app.on_load($handler);
                };
                ("ready", $handler:expr) => {
                    app.on_ready($handler);
                };
            }

            macro_rules! CONF {
                (name = $value:expr) => {
                    app.config_mut().set("name", $value)?;
                };
                (version = $value:expr) => {
                    app.config_mut().set("version", $value)?;
                };
                (ip = $value:expr) => {
                    app.config_mut().set("ip", $value)?;
                };
                (port = $value:expr) => {
                    app.config_mut().set("port", $value)?;
                };
                ($name:expr, $value:expr) => {
                    app.config_mut().set($name, $value)?;
                };
            }

            macro_rules! PLUGIN {
                ($id:expr, $plugin:expr) => {
                    app.plugin($id, $plugin);
                };
            }

            macro_rules! FLOWSTREAM {
                ($id:expr, $flow_body:block) => {
                    {
                        let flow = app.flowstream($id);
                        macro_rules! FLOW_INPUT {
                            ($name:expr, $handler:expr) => {
                                flow.input($name, $handler);
                            };
                        }
                        macro_rules! FLOW_RPC {
                            ($name:expr, $handler:expr) => {
                                flow.rpc($name, $handler);
                            };
                        }
                        $flow_body
                    }
                };
            }

            $($body)*
            Ok(())
        }
    };
}

/// Starts a convention-driven Total.rs application.
///
/// The generated module is created by `total5::build::discover()` from a
/// project's `build.rs`.
#[macro_export]
macro_rules! main {
    () => {
        include!(concat!(env!("OUT_DIR"), "/total5_app.rs"));

        #[tokio::main]
        async fn main() -> Result<(), ::total5::Error> {
            let mut app = ::total5::Total::convention()?;
            total5_generated::install(&mut app)?;
            app.run().await
        }
    };
}

/// Starts a convention-driven Total.rs application in development mode.
///
/// Development mode keeps the same one-line startup as production, but prints
/// discovered framework components and marks the application config as debug.
#[macro_export]
macro_rules! dev {
    () => {
        include!(concat!(env!("OUT_DIR"), "/total5_app.rs"));

        #[tokio::main]
        async fn main() -> Result<(), ::total5::Error> {
            let mut app = ::total5::Total::convention_dev()?;
            total5_generated::install(&mut app)?;
            app.run_dev().await
        }
    };
}

#[cfg(any(feature = "build", test))]
pub mod build {
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};

    pub fn discover() -> io::Result<()> {
        let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").map_err(|err| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("CARGO_MANIFEST_DIR: {err}"),
            )
        })?);
        let out_dir =
            PathBuf::from(std::env::var("OUT_DIR").map_err(|err| {
                io::Error::new(io::ErrorKind::NotFound, format!("OUT_DIR: {err}"))
            })?);
        let source = discover_source(&manifest_dir)?;
        fs::write(out_dir.join("total5_app.rs"), source)
    }

    pub fn discover_source(root: &Path) -> io::Result<String> {
        let src = root.join("src");
        let mut modules = Vec::new();
        discover_dir(&src, "definitions", &mut modules)?;
        discover_dir(&src, "modules", &mut modules)?;
        discover_dir(&src, "services", &mut modules)?;
        discover_dir(&src, "flowstreams", &mut modules)?;
        discover_dir(&src, "schemas", &mut modules)?;
        discover_dir(&src, "controllers", &mut modules)?;
        discover_plugins(&src, &mut modules)?;

        let mut out = String::from("mod total5_generated {\n");
        for module in &modules {
            out.push_str(&format!(
                "    #[path = \"{}\"]\n    mod {};\n",
                module.path.display(),
                module.name
            ));
        }
        out.push_str(
            "    pub fn install(app: &mut ::total5::Total) -> Result<(), ::total5::Error> {\n",
        );
        for module in &modules {
            out.push_str(&format!("        {}::install(app)?;\n", module.name));
        }
        out.push_str("        Ok(())\n    }\n}\n");
        Ok(out)
    }

    fn discover_dir(src: &Path, dir: &str, modules: &mut Vec<Module>) -> io::Result<()> {
        let root = src.join(dir);
        if !root.is_dir() {
            return Ok(());
        }
        let mut files = read_sorted(&root)?;
        files.retain(|path| path.extension().and_then(|value| value.to_str()) == Some("rs"));
        for file in files {
            if file.file_name().and_then(|value| value.to_str()) == Some("mod.rs") {
                continue;
            }
            push_if_installable(dir, file, modules)?;
        }
        Ok(())
    }

    fn discover_plugins(src: &Path, modules: &mut Vec<Module>) -> io::Result<()> {
        let root = src.join("plugins");
        if !root.is_dir() {
            return Ok(());
        }
        for plugin in read_sorted(&root)? {
            if !plugin.is_dir() {
                continue;
            }
            for name in ["mod.rs", "index.rs"] {
                let file = plugin.join(name);
                if file.is_file() {
                    let plugin_name = plugin
                        .file_name()
                        .and_then(|value| value.to_str())
                        .unwrap_or("plugin");
                    push_if_installable(&format!("plugins_{plugin_name}"), file, modules)?;
                    break;
                }
            }
        }
        Ok(())
    }

    fn push_if_installable(
        prefix: &str,
        path: PathBuf,
        modules: &mut Vec<Module>,
    ) -> io::Result<()> {
        println!("cargo:rerun-if-changed={}", path.display());
        let source = fs::read_to_string(&path)?;
        if !source.contains("fn install") && !source.contains("INSTALL!") {
            return Ok(());
        }
        let stem = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("module");
        modules.push(Module {
            name: module_name(prefix, stem),
            path: path.canonicalize()?,
        });
        Ok(())
    }

    fn read_sorted(path: &Path) -> io::Result<Vec<PathBuf>> {
        println!("cargo:rerun-if-changed={}", path.display());
        let mut output = fs::read_dir(path)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        output.sort();
        Ok(output)
    }

    fn module_name(prefix: &str, stem: &str) -> String {
        let raw = format!("{prefix}_{stem}");
        raw.chars()
            .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
            .collect()
    }

    struct Module {
        name: String,
        path: PathBuf,
    }
}

#[cfg(test)]
mod tests;
