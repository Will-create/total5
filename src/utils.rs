// src/utils.rs
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::fs;
use tokio;

pub struct TPath {
    base_dir: PathBuf,
    temporary_dirs: HashMap<String, PathBuf>,
}

impl TPath {
    pub fn new(base_dir: PathBuf) -> Self {
        let mut temp_dirs = HashMap::new();
        // Initialize with default directories
        temp_dirs.insert("logs".to_string(), base_dir.join("logs"));
        temp_dirs.insert("scripts".to_string(), base_dir.join("scripts"));
        temp_dirs.insert("public".to_string(), base_dir.join("public"));
        temp_dirs.insert("private".to_string(), base_dir.join("private"));
        temp_dirs.insert("databases".to_string(), base_dir.join("databases"));
        temp_dirs.insert("plugins".to_string(), base_dir.join("plugins"));
        temp_dirs.insert("templates".to_string(), base_dir.join("templates"));
        temp_dirs.insert("flowstreams".to_string(), base_dir.join("flowstreams"));
        temp_dirs.insert("modules".to_string(), base_dir.join("modules"));
        temp_dirs.insert("tmp".to_string(), base_dir.join("tmp"));
        
        Self {
            base_dir,
            temporary_dirs: temp_dirs,
        }
    }
    
    // Path utility methods
    pub fn root(&self, path: Option<&str>) -> PathBuf {
        match path {
            Some(p) => self.base_dir.join(p),
            None => self.base_dir.clone(),
        }
    }
    
    pub fn logs(&self, path: Option<&str>) -> PathBuf {
        let logs_dir = self.temporary_dirs.get("logs").unwrap();
        match path {
            Some(p) => logs_dir.join(p),
            None => logs_dir.clone(),
        }
    }
    
    pub fn scripts(&self, path: Option<&str>) -> PathBuf {
        let scripts_dir = self.temporary_dirs.get("scripts").unwrap();
        match path {
            Some(p) => scripts_dir.join(p),
            None => scripts_dir.clone(),
        }
    }
    
    pub fn public(&self, path: Option<&str>) -> PathBuf {
        let public_dir = self.temporary_dirs.get("public").unwrap();
        match path {
            Some(p) => public_dir.join(p),
            None => public_dir.clone(),
        }
    }
    
    pub fn private(&self, path: Option<&str>) -> PathBuf {
        let private_dir = self.temporary_dirs.get("private").unwrap();
        match path {
            Some(p) => private_dir.join(p),
            None => private_dir.clone(),
        }
    }
    
    pub fn databases(&self, path: Option<&str>) -> PathBuf {
        let databases_dir = self.temporary_dirs.get("databases").unwrap();
        match path {
            Some(p) => databases_dir.join(p),
            None => databases_dir.clone(),
        }
    }
    
    pub fn plugins(&self, path: Option<&str>) -> PathBuf {
        let plugins_dir = self.temporary_dirs.get("plugins").unwrap();
        match path {
            Some(p) => plugins_dir.join(p),
            None => plugins_dir.clone(),
        }
    }
    
    pub fn templates(&self, path: Option<&str>) -> PathBuf {
        let templates_dir = self.temporary_dirs.get("templates").unwrap();
        match path {
            Some(p) => templates_dir.join(p),
            None => templates_dir.clone(),
        }
    }
    
    pub fn flowstreams(&self, path: Option<&str>) -> PathBuf {
        let flowstreams_dir = self.temporary_dirs.get("flowstreams").unwrap();
        match path {
            Some(p) => flowstreams_dir.join(p),
            None => flowstreams_dir.clone(),
        }
    }
    
    pub fn modules(&self, path: Option<&str>) -> PathBuf {
        let modules_dir = self.temporary_dirs.get("modules").unwrap();
        match path {
            Some(p) => modules_dir.join(p),
            None => modules_dir.clone(),
        }
    }
    
    pub fn directory(&self, dir_type: &str, path: Option<&str>) -> PathBuf {
        if let Some(dir) = self.temporary_dirs.get(dir_type) {
            match path {
                Some(p) => dir.join(p),
                None => dir.clone(),
            }
        } else {
            self.base_dir.clone() // Fallback to base directory
        }
    }
    
    pub fn tmp(&self, path: Option<&str>) -> PathBuf {
        let tmp_dir = self.temporary_dirs.get("tmp").unwrap();
        match path {
            Some(p) => tmp_dir.join(p),
            None => tmp_dir.clone(),
        }
    }
    
    pub fn temp(&self, path: Option<&str>) -> PathBuf {
        self.tmp(path) // Alias for tmp
    }
    
    pub async fn exists(&self, path: &Path) -> (bool, u64, bool) {
        match tokio::fs::metadata(path).await {
            Ok(metadata) => (true, metadata.len(), metadata.is_file()),
            Err(_) => (false, 0, false),
        }
    }
    
    // Private join method
    pub fn join(&self, directory: &Path, path: &str) -> PathBuf {
        self.verify(directory);
        directory.join(path)
    }
    
    // Route handling 
    pub fn route(&self, path: &str, directory: &str) -> PathBuf {
        // Absolute path
        if path.starts_with('~') {
            return PathBuf::from(&path[1..]);
        }
        
        // Plugin paths
        if path.starts_with('_') {
            let tmp = &path[1..];
            if let Some(index) = tmp.find('/') {
                let plugin_name = &tmp[..index];
                let dir_part = if directory == "root" { "" } else { directory };
                let path_part = &tmp[(index + 2)..];
                return self.plugins(Some(&format!("{}/{}/{}", plugin_name, dir_part, path_part)));
            }
        }
        
        // Regular directory paths
        match directory {
            "root" => self.root(Some(path)),
            "logs" => self.logs(Some(path)),
            "scripts" => self.scripts(Some(path)),
            "public" => self.public(Some(path)),
            "private" => self.private(Some(path)),
            "databases" => self.databases(Some(path)),
            "plugins" => self.plugins(Some(path)),
            "templates" => self.templates(Some(path)),
            "flowstreams" => self.flowstreams(Some(path)),
            "modules" => self.modules(Some(path)),
            "tmp" => self.tmp(Some(path)),
            _ => self.directory(directory, Some(path)),
        }
    }
    
    // File system utilities
    pub async fn unlink(&self, path: &Path) -> std::io::Result<()> {
        tokio::fs::remove_file(path).await
    }
    
    pub async fn rmdir(&self, path: &Path) -> std::io::Result<()> {
        tokio::fs::remove_dir_all(path).await
    }
    
    // Path verification
    pub fn verify(&self, path: &Path) {
        self.mkdir(path);
    }
    
    // Directory creation
    pub fn mkdir(&self, path: &Path) {
        if !path.exists() {
            let _ = fs::create_dir_all(path);
        }
    }

    pub fn exists_dir(&self, path: &Path) -> bool {
        path.exists()
    }
}
