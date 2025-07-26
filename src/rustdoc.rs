use anyhow::{Result, anyhow};
use cargo_metadata::{Metadata, MetadataCommand};
use cargo_toml::Manifest;
use fieldwork::Fieldwork;
use rustdoc_types::{Crate, FORMAT_VERSION, Id, Item};
use serde::Deserialize;
use std::fmt::{self, Debug, Formatter};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;
use walkdir::WalkDir;

mod crate_name;

use crate::doc_ref::{self, DocRef};
use crate::request::Request;
use crate_name::CrateName;

pub(crate) const RUST_CRATES: [CrateName<'_>; 5] = [
    CrateName("std"),
    CrateName("alloc"),
    CrateName("core"),
    CrateName("proc_macro"),
    CrateName("test"),
];

/// Manages a Cargo project and its rustdoc JSON files
#[derive(Fieldwork)]
#[fieldwork(get)]
pub(crate) struct RustdocProject {
    manifest_path: PathBuf,
    target_dir: PathBuf,
    manifest: Manifest,
    metadata: Metadata,
    crate_info: Vec<CrateInfo>,
    workspace_packages: Box<[String]>,
    rustc_docs: Option<(PathBuf, String)>,
}

impl Debug for RustdocProject {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("RustdocProject")
            .field("manifest_path", &self.manifest_path)
            .field("target_dir", &self.target_dir)
            .field("crate_info", &self.crate_info)
            .finish_non_exhaustive()
    }
}

pub(crate) fn rustc_docs() -> Option<(PathBuf, String)> {
    let sysroot = Command::new("rustup")
        .args(["run", "nightly", "rustc", "--print", "sysroot"])
        .output()
        .ok()?;

    if !sysroot.status.success() {
        return None;
    }

    let s = str::from_utf8(&sysroot.stdout).ok()?;

    let path = PathBuf::from(s.trim()).join("share/doc/rust/json/");

    let version = Command::new("rustup")
        .args(["run", "nightly", "rustc", "--version", "--verbose"])
        .arg("run")
        .output()
        .ok()?;

    if !version.status.success() {
        return None;
    }

    let version = str::from_utf8(&version.stdout)
        .ok()?
        .lines()
        .find_map(|line| line.strip_prefix("release: "))?
        .to_string();

    path.exists().then_some((path, version))
}

fn eq_ignoring_dash_underscore(a: &str, b: &str) -> bool {
    let mut a = a.chars();
    let mut b = b.chars();
    loop {
        match (a.next(), b.next()) {
            (Some('_'), Some('-')) | (Some('-'), Some('_')) => {}
            (Some(a), Some(b)) if a == b => {}
            (None, None) => break true,
            _ => break false,
        }
    }
}

impl RustdocProject {
    /// Create a new project from a Cargo.toml path
    pub(crate) fn load(manifest_path: PathBuf) -> Result<Self> {
        // Look for Cargo.toml in the working directory
        if !manifest_path.exists() {
            return Err(anyhow!(
                "Not a Rust project: Cargo.toml not found in {}",
                manifest_path.display()
            ));
        }

        let manifest = Manifest::from_path(&manifest_path)?;
        let project_root = manifest_path
            .parent()
            .ok_or_else(|| anyhow!("Invalid manifest path"))?;

        let target_dir = project_root.join("target");

        let metadata = MetadataCommand::new()
            .manifest_path(&manifest_path)
            .exec()?;

        let workspace_packages = metadata
            .workspace_packages()
            .iter()
            .map(|package| package.name.to_string())
            .collect();

        let rustc_docs = rustc_docs();

        let mut project = Self {
            manifest_path,
            manifest,
            target_dir,
            metadata,
            crate_info: vec![],
            workspace_packages,
            rustc_docs,
        };

        project.crate_info = project.generate_crate_info();
        Ok(project)
    }

    pub(crate) fn resolve_json_path<'a>(
        &'a self,
        crate_name: CrateName<'a>,
    ) -> Option<(PathBuf, CrateType)> {
        let doc_dir = self.target_dir.join("doc");

        if RUST_CRATES.contains(&crate_name)
            && let Some((rustc_docs, _)) = &self.rustc_docs
        {
            Some((
                rustc_docs.join(format!("{crate_name}.json")),
                CrateType::Rust,
            ))
        } else if self.available_crates().contains(&crate_name) {
            let underscored = crate_name.replace('-', "_");
            Some((
                doc_dir.join(format!("{underscored}.json")),
                if self.is_workspace_package(crate_name) {
                    CrateType::Workspace
                } else {
                    CrateType::Library
                },
            ))
        } else {
            None
        }
    }

    pub(crate) fn is_workspace_package(&self, crate_name: CrateName<'_>) -> bool {
        self.workspace_packages
            .iter()
            .any(|c| eq_ignoring_dash_underscore(c, &crate_name))
    }

    /// Generate documentation for the project or a specific package
    pub(crate) fn rebuild_docs(&self, crate_name: CrateName<'_>) -> Result<()> {
        let project_root = self.project_root();

        let output = Command::new("rustup")
            .arg("run")
            .args([
                "nightly",
                "cargo",
                "doc",
                "--no-deps",
                "--package",
                &*crate_name,
            ])
            .env("RUSTDOCFLAGS", "-Z unstable-options --output-format=json")
            .current_dir(project_root)
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("cargo doc failed: {}", stderr));
        }
        Ok(())
    }

    /// Get available crate names and optional descriptions
    fn generate_crate_info(&self) -> Vec<CrateInfo> {
        let mut crates = vec![];
        let default_crate = self.default_crate_name();

        for package in self.metadata.workspace_packages() {
            crates.push(CrateInfo {
                crate_type: CrateType::Workspace,
                name: package.name.to_string(),
                description: package.description.clone(),
                version: Some(package.version.to_string()),
                dev_dep: false,
                default_crate: default_crate
                    .is_some_and(|dc| eq_ignoring_dash_underscore(&dc, &package.name)),
            });
        }

        for (crate_names, dev_dep) in [
            (self.manifest.dependencies.keys(), false),
            (self.manifest.dev_dependencies.keys(), true),
        ] {
            for crate_name in crate_names {
                let metadata = self
                    .metadata
                    .packages
                    .iter()
                    .find(|package| eq_ignoring_dash_underscore(&package.name, crate_name));
                crates.push(CrateInfo {
                    crate_type: CrateType::Library,
                    version: metadata.map(|p| p.version.to_string()),
                    description: metadata.and_then(|p| p.description.clone()),
                    dev_dep,
                    name: crate_name.clone(),
                    default_crate: false,
                });
            }
        }

        if let Some((_, rustc_version)) = self.rustc_docs() {
            crates.extend([
                ("std", "The Rust Standard Library"),
                ("alloc","The Rust core allocation and collections library"),
                ("core", "The Rust Core Library"),
                ("proc_macro", "A support library for macro authors when defining new macros"),
                ("test", "Support code for rustc's built in unit-test and micro-benchmarking framework")
            ].map(|(name, description)|{
                CrateInfo {
                    crate_type: CrateType::Rust,
                    version: Some(rustc_version.to_string()),
                    description: Some(description.to_string()),
                    dev_dep: false,
                    name: name.to_string(),
                    default_crate: false
                }})
            );
        }

        crates
    }

    /// Get available crate names and optional descriptions
    pub(crate) fn available_crates(&self) -> Vec<CrateName<'_>> {
        self.manifest
            .dependencies
            .keys()
            .chain(self.manifest.dev_dependencies.keys())
            .chain(self.metadata.workspace_packages().iter().map(|x| &*x.name))
            .filter_map(|name| CrateName::new(name))
            .collect()
    }

    pub(crate) fn project_root(&self) -> &Path {
        self.manifest_path.parent().unwrap_or(&self.manifest_path)
    }

    pub(crate) fn default_crate_name(&self) -> Option<CrateName<'_>> {
        if let Some(root) = self.metadata.root_package() {
            CrateName::new(&root.name)
        } else {
            self.metadata
                .workspace_default_packages()
                .first()
                .and_then(|p| CrateName::new(p.name.as_str()))
        }
    }

    pub(crate) fn normalize_crate_name<'a>(&'a self, crate_name: &str) -> Option<CrateName<'a>> {
        match crate_name {
            "crate" => self.default_crate_name(),

            // rustdoc placeholders
            "alloc" | "alloc_crate" => Some(CrateName("alloc")),
            "core" | "core_crate" => Some(CrateName("core")),
            "proc_macro" | "proc_macro_crate" => Some(CrateName("proc_macro")),
            "test" | "test_crate" => Some(CrateName("test")),
            "std" | "std_crate" => Some(CrateName("std")),
            "std_detect" | "rustc_literal_escaper" => None,

            // future-proof: skip internal rustc crates
            name if name.starts_with("rustc_") => None,
            name => self
                .available_crates()
                .iter()
                .find(|correct_name| eq_ignoring_dash_underscore(correct_name, name))
                .copied(),
        }
    }

    /// Load rustdoc data for a specific crate
    pub(crate) fn load_crate(&self, crate_name: CrateName<'_>) -> Option<RustdocData> {
        let (json_path, crate_type) = self.resolve_json_path(crate_name)?;

        match crate_type {
            CrateType::Workspace => self.load_workspace(crate_name, &json_path),
            CrateType::Library => self.load_dep(crate_name, &json_path),
            CrateType::Rust => self.load_rustc(crate_name, &json_path),
        }
    }

    pub(crate) fn load_dep(
        &self,
        crate_name: CrateName<'_>,
        json_path: &Path,
    ) -> Option<RustdocData> {
        let mut tried_rebuilding = false;
        let expected_version = self
            .metadata
            .packages
            .iter()
            .find(|x| **x.name == *crate_name)
            .map(|x| x.version.to_string());

        loop {
            if let Ok(content) = std::fs::read_to_string(json_path)
                && let Ok(RustdocVersion {
                    format_version,
                    crate_version,
                }) = serde_json::from_str(&content)
                && format_version == FORMAT_VERSION
                && crate_version == expected_version
            {
                let crate_data: Crate = serde_json::from_str(&content).ok()?;

                break Some(RustdocData {
                    crate_data,
                    name: crate_name.to_string(),
                    crate_type: CrateType::Library,
                });
            } else if !tried_rebuilding {
                tried_rebuilding = true;
                if self.rebuild_docs(crate_name).is_ok() {
                    continue;
                }
            }
            break None;
        }
    }

    fn load_rustc(&self, crate_name: CrateName<'_>, json_path: &Path) -> Option<RustdocData> {
        if let Ok(content) = std::fs::read_to_string(json_path)
            && let Ok(RustdocVersion { format_version, .. }) = serde_json::from_str(&content)
            && format_version == FORMAT_VERSION
        {
            let crate_data: Crate = serde_json::from_str(&content).ok()?;

            Some(RustdocData {
                crate_data,
                name: crate_name.to_string(),
                crate_type: CrateType::Library,
            })
        } else {
            None
        }
    }

    fn load_workspace(&self, crate_name: CrateName<'_>, json_path: &Path) -> Option<RustdocData> {
        let mut tried_rebuilding = false;
        loop {
            let needs_rebuild = json_path
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .is_none_or(|docs_updated| {
                    WalkDir::new(self.project_root().join("src"))
                        .into_iter()
                        .filter_map(|entry| -> Option<SystemTime> {
                            entry.ok()?.metadata().ok()?.modified().ok()
                        })
                        .any(|file_updated| file_updated > docs_updated)
                });

            if !needs_rebuild
                && let Ok(content) = std::fs::read_to_string(json_path)
                && let Ok(RustdocVersion { format_version, .. }) = serde_json::from_str(&content)
                && format_version == FORMAT_VERSION
            {
                let crate_data: Crate = serde_json::from_str(&content).ok()?;

                break Some(RustdocData {
                    crate_data,
                    name: crate_name.to_string(),
                    crate_type: CrateType::Library,
                });
            } else if !tried_rebuilding {
                tried_rebuilding = true;
                if self.rebuild_docs(crate_name).is_ok() {
                    continue;
                }
            }
            break None;
        }
    }
}

#[derive(Debug, Fieldwork)]
#[fieldwork(get, rename_predicates)]
pub(crate) struct CrateInfo {
    crate_type: CrateType,
    version: Option<String>,
    description: Option<String>,
    dev_dep: bool,
    name: String,
    default_crate: bool,
}

#[derive(Deserialize, Debug)]
struct RustdocVersion {
    format_version: u32,
    crate_version: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) enum CrateType {
    Workspace,
    Library,
    Rust,
}
impl CrateType {
    pub(crate) fn is_workspace(&self) -> bool {
        matches!(self, Self::Workspace)
    }
}

/// Wrapper around rustdoc JSON data that provides convenient query methods
#[derive(Clone, Fieldwork)]
#[fieldwork(get, rename_predicates)]
pub(crate) struct RustdocData {
    crate_data: Crate,

    name: String,

    crate_type: CrateType,
}

impl Debug for RustdocData {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("RustdocData")
            .field("name", &self.name)
            .field("crate_type", &self.crate_type)
            .finish()
    }
}

impl Deref for RustdocData {
    type Target = Crate;

    fn deref(&self) -> &Self::Target {
        &self.crate_data
    }
}

impl RustdocData {
    pub(crate) fn get<'a>(&'a self, request: &'a Request, id: &Id) -> Option<DocRef<'a, Item>> {
        let item = self.crate_data.index.get(id)?;
        Some(DocRef::new(request, self, item))
    }

    pub(crate) fn path<'a>(&'a self, id: &Id) -> Option<doc_ref::Path<'a>> {
        self.paths.get(id).map(|summary| summary.into())
    }
}
