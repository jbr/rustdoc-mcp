use anyhow::{Result, anyhow};
use cargo_metadata::{Metadata, MetadataCommand};
use cargo_toml::Manifest;
use fieldwork::Fieldwork;
use rustdoc_types::{Crate, FORMAT_VERSION, Id, Item};
use serde::Deserialize;
use std::collections::HashMap;
use std::fmt::{self, Debug, Formatter};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;
use walkdir::WalkDir;

mod crate_name;

use crate::doc_ref::DocRef;
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
    descriptions: HashMap<String, String>,
    workspace_packages: Box<[String]>,
    rustc_docs: Option<(PathBuf, String)>,
}

impl Debug for RustdocProject {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("RustdocProject")
            .field("manifest_path", &self.manifest_path)
            .field("target_dir", &self.target_dir)
            .field("descriptions", &self.descriptions)
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
            descriptions: HashMap::new(),
            workspace_packages,
            rustc_docs,
        };

        project.populate_descriptions();
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
                if self
                    .workspace_packages
                    .iter()
                    .any(|c| eq_ignoring_dash_underscore(c, &crate_name))
                {
                    CrateType::Workspace
                } else {
                    CrateType::Library
                },
            ))
        } else {
            None
        }
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

    fn populate_descriptions(&mut self) {
        self.descriptions = self
            .metadata
            .packages
            .iter()
            .filter_map(|package| {
                package
                    .description
                    .as_ref()
                    .map(|description| (package.name.to_string(), description.clone()))
            })
            .chain(if let Some((_, rustc_version)) = self.rustc_docs() {
                vec![
                    ("std".to_string(), format!("The Rust Standard Library (rustc {rustc_version})")),
                    (
                        "alloc".to_string(),
                        format!("The Rust core allocation and collections library (rustc {rustc_version})"),
                    ),
                    ("core".to_string(), format!("The Rust Core Library (rustc {rustc_version})")),
                    (
                        "proc_macro".to_string(),
                        format!("A support library for macro authors when defining new macros (rustc {rustc_version})"),
                    ),
                    (
                        "test".to_string(),
                        format!("Support code for rustc's built in unit-test and micro-benchmarking framework (rustc {rustc_version})")
                    ),
                ]
            } else {
                vec![]
            })
            .collect();
    }

    /// Get available crate names and optional descriptions
    pub(crate) fn available_crates_with_descriptions(&self) -> Vec<(String, Option<String>)> {
        self.manifest
            .dependencies
            .keys()
            .chain(self.manifest.dev_dependencies.keys())
            .map(|x| &**x)
            .chain(
                if self.rustc_docs.is_some() {
                    RUST_CRATES.as_slice()
                } else {
                    [].as_slice()
                }
                .iter()
                .map(|x| &**x),
            )
            .map(|name| (name.to_string(), self.descriptions.get(name).cloned()))
            .chain(
                self.metadata
                    .workspace_packages()
                    .iter()
                    .map(|x| (x.name.to_string(), x.description.clone())),
            )
            .collect()
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
}
