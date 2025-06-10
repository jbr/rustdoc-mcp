use anyhow::Result;
use rustdoc_types::{Crate, Id, Item};
use serde_json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Manages a Cargo project and its rustdoc JSON files
pub struct RustdocProject {
    manifest_path: PathBuf,
    target_dir: PathBuf,
    available_crates: HashMap<String, PathBuf>, // crate name -> json file path
}

impl RustdocProject {
    /// Create a new project from a Cargo.toml path
    pub fn from_manifest<P: AsRef<Path>>(manifest_path: P) -> Result<Self> {
        let manifest_path = manifest_path.as_ref().to_path_buf();
        let project_root = manifest_path.parent()
            .ok_or_else(|| anyhow::anyhow!("Invalid manifest path"))?;
        
        let target_dir = project_root.join("target");
        
        let mut project = Self {
            manifest_path,
            target_dir,
            available_crates: HashMap::new(),
        };
        
        project.discover_crates()?;
        Ok(project)
    }
    
    /// Discover available crate documentation
    fn discover_crates(&mut self) -> Result<()> {
        let doc_dir = self.target_dir.join("doc");
        if !doc_dir.exists() {
            return Ok(()); // No docs generated yet
        }
        
        self.available_crates.clear();
        
        // Scan for .json files in target/doc/
        for entry in std::fs::read_dir(&doc_dir)? {
            let entry = entry?;
            let path = entry.path();
            
            if let Some(extension) = path.extension() {
                if extension == "json" {
                    if let Some(stem) = path.file_stem() {
                        if let Some(crate_name) = stem.to_str() {
                            self.available_crates.insert(crate_name.to_string(), path);
                        }
                    }
                }
            }
        }
        
        Ok(())
    }
    
    /// Generate documentation for the project or a specific package
    pub fn generate_docs(&mut self, package: Option<&str>, rebuild: bool) -> Result<()> {
        if !rebuild {
            return Ok(());
        }
        
        let project_root = self.manifest_path.parent()
            .ok_or_else(|| anyhow::anyhow!("Invalid manifest path"))?;
        
        let mut cmd = Command::new("cargo");
        cmd.arg("+nightly")
           .arg("doc")
           .env("RUSTDOCFLAGS", "-Z unstable-options --output-format=json")
           .current_dir(project_root);
        
        if let Some(pkg) = package {
            cmd.arg("--package").arg(pkg);
        }
        
        let output = cmd.output()?;
        
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("cargo doc failed: {}", stderr));
        }
        
        // Rediscover crates after rebuild
        self.discover_crates()?;
        Ok(())
    }
    
    /// Get available crate names
    pub fn available_crates(&self) -> Vec<&String> {
        self.available_crates.keys().collect()
    }
    
    /// Load rustdoc data for a specific crate
    pub fn load_crate(&self, crate_name: &str) -> Result<RustdocData> {
        let json_path = self.available_crates.get(crate_name)
            .ok_or_else(|| anyhow::anyhow!("Crate '{}' not found. Available crates: {:?}", 
                crate_name, self.available_crates.keys().collect::<Vec<_>>()))?;
        
        RustdocData::from_file(json_path)
    }
    
    /// Get project information
    pub fn project_info(&self) -> ProjectInfo {
        ProjectInfo {
            manifest_path: self.manifest_path.clone(),
            target_dir: self.target_dir.clone(),
            available_crates: self.available_crates.keys().cloned().collect(),
        }
    }
}

/// Information about a cargo project
#[derive(Debug, Clone)]
pub struct ProjectInfo {
    pub manifest_path: PathBuf,
    pub target_dir: PathBuf,
    pub available_crates: Vec<String>,
}

/// Wrapper around rustdoc JSON data that provides convenient query methods
pub struct RustdocData {
    crate_data: Crate,
}

impl RustdocData {
    /// Load rustdoc JSON from a file path
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let crate_data: Crate = serde_json::from_str(&content)?;
        Ok(Self { crate_data })
    }

    /// Get basic crate information
    pub fn crate_info(&self) -> CrateInfo {
        CrateInfo {
            format_version: self.crate_data.format_version,
            crate_version: self.crate_data.crate_version.clone(),
            includes_private: self.crate_data.includes_private,
            root_id: self.crate_data.root,
            item_count: self.crate_data.index.len(),
            external_crates: self.crate_data.external_crates.clone(),
        }
    }

    /// Get an item by its ID
    pub fn get_item(&self, id: &Id) -> Option<&Item> {
        self.crate_data.index.get(id)
    }

    /// Get the root item
    pub fn root_item(&self) -> Option<&Item> {
        self.get_item(&self.crate_data.root)
    }

    /// List all items of a specific kind
    pub fn items_by_kind(&self, kind: &str) -> Vec<(&Id, &Item)> {
        self.crate_data
            .index
            .iter()
            .filter(|(_, item)| item.inner.kind_name() == kind)
            .collect()
    }

    /// Search items by name (case-insensitive substring match)
    pub fn search_items(&self, query: &str) -> Vec<(&Id, &Item)> {
        let query_lower = query.to_lowercase();
        self.crate_data
            .index
            .iter()
            .filter(|(_, item)| {
                item.name
                    .as_ref()
                    .map(|name| name.to_lowercase().contains(&query_lower))
                    .unwrap_or(false)
            })
            .collect()
    }

    /// Get all available item kinds in this crate
    pub fn available_kinds(&self) -> Vec<String> {
        let mut kinds: Vec<String> = self
            .crate_data
            .index
            .values()
            .map(|item| item.inner.kind_name().to_string())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        kinds.sort();
        kinds
    }

    /// Get summary statistics about item kinds
    pub fn kind_statistics(&self) -> HashMap<String, usize> {
        let mut stats = HashMap::new();
        for item in self.crate_data.index.values() {
            let kind = item.inner.kind_name().to_string();
            *stats.entry(kind).or_insert(0) += 1;
        }
        stats
    }

    /// Get items that are publicly visible
    pub fn public_items(&self) -> Vec<(&Id, &Item)> {
        self.crate_data
            .index
            .iter()
            .filter(|(_, item)| matches!(item.visibility, rustdoc_types::Visibility::Public))
            .collect()
    }
}

/// Basic information about a crate
#[derive(Debug, Clone)]
pub struct CrateInfo {
    pub format_version: u32,
    pub crate_version: Option<String>,
    pub includes_private: bool,
    pub root_id: Id,
    pub item_count: usize,
    pub external_crates: HashMap<u32, rustdoc_types::ExternalCrate>,
}

/// Extension trait to get kind names from ItemEnum
pub trait ItemKind {
    fn kind_name(&self) -> &'static str;
}

impl ItemKind for rustdoc_types::ItemEnum {
    fn kind_name(&self) -> &'static str {
        match self {
            rustdoc_types::ItemEnum::Module(_) => "module",
            rustdoc_types::ItemEnum::ExternCrate { .. } => "extern_crate",
            rustdoc_types::ItemEnum::Use(_) => "use",
            rustdoc_types::ItemEnum::Union(_) => "union",
            rustdoc_types::ItemEnum::Struct(_) => "struct",
            rustdoc_types::ItemEnum::StructField(_) => "struct_field",
            rustdoc_types::ItemEnum::Enum(_) => "enum",
            rustdoc_types::ItemEnum::Variant(_) => "variant",
            rustdoc_types::ItemEnum::Function(_) => "function",
            rustdoc_types::ItemEnum::TypeAlias(_) => "type_alias",
            rustdoc_types::ItemEnum::Constant { .. } => "constant",
            rustdoc_types::ItemEnum::Trait(_) => "trait",
            rustdoc_types::ItemEnum::TraitAlias(_) => "trait_alias",
            rustdoc_types::ItemEnum::Impl(_) => "impl",
            rustdoc_types::ItemEnum::Static(_) => "static",
            rustdoc_types::ItemEnum::Macro(_) => "macro",
            rustdoc_types::ItemEnum::ProcMacro(_) => "proc_macro",
            rustdoc_types::ItemEnum::Primitive(_) => "primitive",
            rustdoc_types::ItemEnum::AssocConst { .. } => "assoc_const",
            rustdoc_types::ItemEnum::AssocType { .. } => "assoc_type",
            rustdoc_types::ItemEnum::ExternType => "extern_type",
        }
    }
}
