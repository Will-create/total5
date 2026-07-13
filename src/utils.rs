use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Convention-based path helper inspired by Total.js `PATH`.
#[derive(Debug, Clone)]
pub struct TPath {
    base_dir: PathBuf,
    directories: HashMap<&'static str, PathBuf>,
}

impl TPath {
    pub fn new(base_dir: PathBuf) -> Self {
        let mut directories = HashMap::new();
        directories.insert("logs", base_dir.join("logs"));
        directories.insert("scripts", base_dir.join("scripts"));
        directories.insert("public", base_dir.join("public"));
        directories.insert("private", base_dir.join("private"));
        directories.insert("databases", base_dir.join("databases"));
        directories.insert("plugins", base_dir.join("plugins"));
        directories.insert("templates", base_dir.join("templates"));
        directories.insert("flowstreams", base_dir.join("flowstreams"));
        directories.insert("modules", base_dir.join("modules"));
        directories.insert("services", base_dir.join("services"));
        directories.insert("tmp", base_dir.join("tmp"));

        Self {
            base_dir,
            directories,
        }
    }

    pub fn root(&self, path: Option<&str>) -> PathBuf {
        append(&self.base_dir, path)
    }

    pub fn logs(&self, path: Option<&str>) -> PathBuf {
        self.directory("logs", path)
    }

    pub fn scripts(&self, path: Option<&str>) -> PathBuf {
        self.directory("scripts", path)
    }

    pub fn public(&self, path: Option<&str>) -> PathBuf {
        self.directory("public", path)
    }

    pub fn private(&self, path: Option<&str>) -> PathBuf {
        self.directory("private", path)
    }

    pub fn databases(&self, path: Option<&str>) -> PathBuf {
        self.directory("databases", path)
    }

    pub fn plugins(&self, path: Option<&str>) -> PathBuf {
        self.directory("plugins", path)
    }

    pub fn templates(&self, path: Option<&str>) -> PathBuf {
        self.directory("templates", path)
    }

    pub fn flowstreams(&self, path: Option<&str>) -> PathBuf {
        self.directory("flowstreams", path)
    }

    pub fn modules(&self, path: Option<&str>) -> PathBuf {
        self.directory("modules", path)
    }

    pub fn services(&self, path: Option<&str>) -> PathBuf {
        self.directory("services", path)
    }

    pub fn tmp(&self, path: Option<&str>) -> PathBuf {
        self.directory("tmp", path)
    }

    pub fn temp(&self, path: Option<&str>) -> PathBuf {
        self.tmp(path)
    }

    pub fn directory(&self, directory: &str, path: Option<&str>) -> PathBuf {
        let base = self.directories.get(directory).unwrap_or(&self.base_dir);
        append(base, path)
    }

    pub fn set_directory(&mut self, directory: &'static str, path: impl Into<PathBuf>) {
        let path = path.into();
        let path = if path.is_absolute() {
            path
        } else {
            self.base_dir.join(path)
        };
        self.directories.insert(directory, path);
    }

    pub fn route(&self, path: &str, directory: &str) -> PathBuf {
        if let Some(stripped) = path.strip_prefix('~') {
            return PathBuf::from(stripped);
        }

        if let Some(stripped) = path.strip_prefix('_') {
            if let Some((plugin, rest)) = stripped.split_once('/') {
                let dir = if directory == "root" { "" } else { directory };
                return self.plugins(Some(&format!("{plugin}/{dir}/{rest}")));
            }
        }

        if directory == "root" {
            self.root(Some(path))
        } else {
            self.directory(directory, Some(path))
        }
    }

    pub fn verify(&self, path: &Path) -> std::io::Result<()> {
        fs::create_dir_all(path)
    }

    pub fn exists_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }

    pub fn exists_file(&self, path: &Path) -> bool {
        path.is_file()
    }
}

fn append(base: &Path, path: Option<&str>) -> PathBuf {
    match path {
        Some(path) if !path.is_empty() => base.join(path),
        _ => base.to_path_buf(),
    }
}
