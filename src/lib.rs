// Totaljs v5 framework implementation in Rust
// The MIT License
// Copyright 2012-2023 (c) Louis Bertson <louisbertsonpetersirka@gmail.com>
use once_cell::sync::Lazy;
use std::path::PathBuf;
use std::collections::HashMap;
use lazy_static::lazy_static;
use std::sync::RwLock;
use serde_json::json;
use chrono::Utc;
use std::fs;
use std::time::Instant;


mod types;
mod utils;
mod globals;

// Re-export the main components for library users
pub use types::{FrameworkValue, InternalStats, Routes, Temporary, Stats, Config, DEF, AuditData, Message, SuccessResult, ErrorInfo, Controller, Parsers, Validators};
pub use utils::TPath;
pub use globals::{EMPTY_ARRAY, EMPTY_OBJECT, REG_HTTPHTTPS, REG_SKIPERRORS, SOCKETWINDOWS, IGNORE_AUDIT};



/// Main Framework structure

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
    pub path: Lazy<TPath>
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
            path: Lazy::new(|| TPath::new(PathBuf::from("src")))
        }
    }
}

// Create a lazy initialized global CONF
pub static CONF: Lazy<RwLock<Config>> = Lazy::new(|| {
    // Initialize default HTTP file types
    let mut httpfiles = HashMap::new();
    for filetype in ["flac", "jpg", "jpeg", "png", "gif", "ico", "wasm", "js", "mjs", "css", "txt", "xml", 
                    "woff", "woff2", "otf", "ttf", "eot", "svg", "zip", "rar", "pdf", "docx", "xlsx", 
                    "doc", "xls", "html", "htm", "appcache", "manifest", "map", "ogv", "ogg", "mp4", 
                    "mp3", "webp", "webm", "swf", "package", "json", "ui", "md", "m4v", "jsx", "heif", 
                    "heic", "ics", "ts", "m3u8", "wav", "xsd", "xsl", "xslt", "ipynb", "ijsnb", "log"].iter() {
        httpfiles.insert(filetype.to_string(), true);
    }

    RwLock::new(Config {
        // Regular properties
        name: String::from("Total.js"),
        version: String::from("1.0.0"),
        author: String::new(),
        secret: String::new(),  // Replace with actual function call
        secret_encryption: String::new(),
        secret_totalapi: String::new(),
        secret_csrf: String::new(),
        secret_tapi: String::new(),
        secret_tms: String::new(),

        // Properties with $ prefix
        _root: String::new(),
        _cors: String::new(),
        _api: String::from("/api/"),
        _sourcemap: true,
        _httpreqlimit: 0,
        _httpcompress: true,
        _httpetag: String::new(),
        _httpmaxsize: 256,
        _httprangebuffer: 5120,
        _httptimeout: 5,
        _httpfiles: httpfiles,
        _httpchecktypes: true,
        _httpmaxage: 60,
        _httpmaxkeys: 33,
        _httpmaxkey: 25,
        _blacklist: String::new(),
        _xpoweredby: String::from("Total.js"),
        _maxopenfiles: 100,
        _minifyjs: true,
        _minifycss: true,
        _minifyhtml: true,
        _localize: true,
        _port: String::from("auto"),
        _ip: String::from("0.0.0.0"),
        _unixsocket: String::new(),
        _timezone: String::from("utc"),
        _insecure: false,
        _performance: false,
        _filtererrors: true,
        _cleartemp: true,
        _customtitles: false,
        _version: String::new(),
        _clearcache: 10,
        _imageconverter: String::from("gm"),
        _imagememory: 0,
        _stats: true,
        _npmcache: String::from("/var/www/.npm"),
        _python: String::from("python3"),
        _wsmaxsize: 256,
        _wscompress: true,
        _wsencodedecode: false,
        _wsmaxlatency: 2000,
        _proxytimeout: 5,
        _cookiesamesite: String::from("Lax"),
        _cookiesecure: false,
        _csrfexpiration: String::from("30 minutes"),
        _tapi: true,
        _tapiurl: String::from("eu"),
        _tapimail: false,
        _tapilogger: false,
        _imprint: true,
        _tms: false,
        _tmsmaxsize: 256,
        _tmsurl: String::from("/$tms/"),
        _tmsclearblocked: 60,
    })
});


impl DEF {
    pub fn new() -> Self {
        let email_regex = regex::Regex::new(r"^[a-zA-Z0-9-_.+]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$").unwrap();
        let url_regex = regex::Regex::new(r"^http(s)?://[^,{}\\]*$").unwrap();
        let phone_regex = regex::Regex::new(r"^[+]?[(]?[0-9]{3}[)]?[-\s.]?[0-9]{3}[-\s.]?[0-9]{4,8}$").unwrap();
        let zip_regex = regex::Regex::new(r"^[0-9a-z\-\s]{3,20}$").unwrap();
        let uid_regex = regex::Regex::new(r"^\d{14,}[a-z]{3}[01]{1}|^\d{9,14}[a-z]{2}[01]{1}a|^\d{4,18}[a-z]{2}\d{1}[01]{1}b|^[0-9a-f]{4,18}[a-z]{2}\d{1}[01]{1}c|^[0-9a-z]{4,18}[a-z]{2}\d{1}[01]{1}d|^[0-9a-zA-Z]{5,10}\d{1}[01]{1}f|^[0-9a-zA-Z]{10}[A-J]{1}r$").unwrap();
        let xss_regex = regex::Regex::new(r"<.*>").unwrap();
        let sqlinjection_regex = regex::Regex::new(r"'(''|[^'])*'|\b(ALTER|CREATE|DELETE|DROP|EXEC(UTE){0,1}|INSERT( +INTO){0,1}|MERGE|SELECT|UPDATE|UNION( +ALL){0,1})\b").unwrap();

        DEF {
            helpers: HashMap::new(),
            currencies: HashMap::new(),
            parsers: Parsers {
                json: Box::new(|value| serde_json::from_str(value)),
                urlencoded: Box::new(|value| {
                    // Simple implementation for URL encoded parsing
                    let mut map = HashMap::new();
                    for pair in value.split('&') {
                        if let Some(idx) = pair.find('=') {
                            let key = &pair[0..idx];
                            let val = &pair[(idx + 1)..];
                            map.insert(key.to_string(), val.to_string());
                        }
                    }
                    map
                }),
                xml: Box::new(|value| {
                    // Simple placeholder for XML parsing
                    Ok(value.to_string())
                }),
            },
            validators: Validators {
                email: email_regex,
                url: url_regex, 
                phone: phone_regex,
                zip: zip_regex,
                uid: uid_regex,
                xss: xss_regex,
                sqlinjection: sqlinjection_regex,
            }
        }
    }

    pub fn on_success<T>(&self, value: T) -> SuccessResult<T> {
        SuccessResult {
            success: true,
            value,
        }
    }

    pub fn on_csrf_create(&self, ctrl: &Controller, f: &F) -> String {
        let config = f.config.lock().unwrap();
        
        if let Some(secret) = &config._csrfexpiration {
            let now = Utc::now();
            let expiration = now + chrono::Duration::milliseconds(secret.as_millis() as i64);
            
            let data = vec![
                ctrl.ip.clone(),
                hash_user_agent(&ctrl.headers.get("user-agent").unwrap_or(&String::new())),
                expiration.timestamp_millis(),
            ];
            
            if let Some(secret_csrf) = &config.secret_csrf {
                encrypt(&serde_json::to_string(&data).unwrap(), secret_csrf)
            } else {
                String::new()
            }
        } else {
            String::new()
        }
    }

    pub fn on_csrf_check(&self, ctrl: &Controller, f: &F) -> bool {
        let config = f.config.lock().unwrap();
        
        if let Some(secret_csrf) = &config.secret_csrf {
            let token = ctrl.headers.get("x-csrf-token").or_else(|| ctrl.query.get("csrf"));
            
            if let Some(token) = token {
                if token.len() > 10 {
                    if let Some(decrypted) = decrypt(token, secret_csrf) {
                        if let Ok(data) = serde_json::from_str::<Vec<String>>(&decrypted) {
                            let now = Utc::now().timestamp_millis();
                            let user_agent_hash = hash_user_agent(&ctrl.headers.get("user-agent").unwrap_or(&String::new()));
                            
                            return data.len() >= 3 && 
                                   data[0] == ctrl.ip && 
                                   data[2].parse::<i64>().unwrap_or(0) >= now && 
                                   data[1] == user_agent_hash;
                        }
                    }
                }
            }
            false
        } else {
            true
        }
    }

    pub fn on_audit(&self, name: Option<&str>, data: &mut AuditData, f: &F) {
        f.stats.performance.open += 1;
        
        data.dtcreated = Utc::now();
        
        let audit_name = name.unwrap_or("audit");
        let log_path = f.path.logs(Some(&format!("{}.log", audit_name)), f);
        
        let serialized = serde_json::to_string(data).unwrap() + "\n";
        let _ = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)
            .and_then(|mut file| std::io::Write::write_all(&mut file, serialized.as_bytes()));
    }

    pub fn on_mail(&self, email: &str, subject: &str, body: &str, 
                   callback: Option<&str>, reply: Option<&str>, f: &F) -> Message {
        let mut msg = Message {
            subject: subject.to_string(),
            body: body.to_string(),
            to_addresses: Vec::new(),
            from_address: None,
            from_name: None,
            reply_to: None,
            cc: Vec::new(),
            bcc: Vec::new(),
            _sending: None,
        };

        // Parse email addresses
        if email.contains(',') {
            for addr in email.split(',') {
                if !addr.trim().is_empty() {
                    msg.to_addresses.push(addr.trim().to_string());
                }
            }
        } else {
            msg.to_addresses.push(email.to_string());
        }

        // Set from address
        let config = f.config.lock().unwrap();
        let from_email = config.mail_from.clone()
            .or_else(|| config.smtp.from.clone())
            .or_else(|| config.smtp.user.clone())
            .unwrap_or_default();
            
        let from_name = config.mail_from_name.clone()
            .or_else(|| config.smtp.name.clone())
            .unwrap_or_default();
            
        msg.from_address = Some(from_email);
        msg.from_name = Some(from_name);
        
        // Handle reply-to
        if let Some(reply_addr) = reply {
            msg.reply_to = Some(reply_addr.to_string());
        } else if let Some(mail_reply) = &config.mail_reply {
            if mail_reply.len() > 3 {
                msg.reply_to = Some(mail_reply.clone());
            }
        }
        
        // Handle CC
        if let Some(mail_cc) = &config.mail_cc {
            if mail_cc.len() > 3 {
                msg.cc.push(mail_cc.clone());
            }
        }
        
        // Handle BCC
        if let Some(mail_bcc) = &config.mail_bcc {
            if mail_bcc.len() > 3 {
                msg.bcc.push(mail_bcc.clone());
            }
        }
        
        // Set sending flag
        msg._sending = Some(Instant::now());
        
        msg
    }

    pub fn on_view_compile(&self, name: &str, html: &str) -> String {
        html.to_string()
    }

    pub fn on_error(&self, err: &dyn std::error::Error, name: Option<&str>, url: Option<&str>, f: &F) {
        let now = Utc::now();
        
        let error_name = if let Some(n) = name {
            n.to_string()
        } else {
            // Try to get name from error if possible
            "unknown".to_string()
        };
        
        let error_str = err.to_string();
        let date_str = now.format("%Y-%m-%d %H:%M:%S").to_string();
        
        let url_info = if let Some(u) = url {
            format!(" ({})", u)
        } else {
            String::new()
        };
        
        println!("ERROR ======= {}: {} ---> {}{}", 
                date_str, 
                if !error_name.is_empty() { format!("{} ---> ", error_name) } else { String::new() },
                error_str,
                url_info);
                
        if let Some(stack) = std::backtrace::Backtrace::capture().to_string() {
            println!("{}", stack);
        }
        
        let mut errors = &mut f.errors;
        let error_info = ErrorInfo {
            error: error_str,
            name: if error_name.is_empty() { None } else { Some(error_name) },
            url: url.map(|u| u.to_string()),
            date: now,
        };
        
        errors.push(error_info);
        if errors.len() > 10 {
            errors.remove(0);
        }
        
        // Handle error event if exists
        // f.$events.error && f.emit('error', obj);
        
        // Update error stats
        f.stats.error += 1;
    }
}

/// Initialize a new framework instance with default values
pub fn initialize_framework() -> Framework {
    Framework::default()
}


/// Create a new PathUtils instance with the given base directory
pub static VERSION: &str = "5.0.0";
pub static F: Lazy<Framework> = Lazy::new(|| Framework::default());
pub static PATH: Lazy<TPath> = Lazy::new(|| TPath::new(PathBuf::from("src")));