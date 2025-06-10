use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};

use crate::rustdoc::{RustdocProject, ItemKind};

/// MCP Server for rustdoc JSON data
pub struct RustdocMcpServer {
    project: Arc<Mutex<Option<RustdocProject>>>,
}

impl RustdocMcpServer {
    pub fn new() -> Self {
        Self {
            project: Arc::new(Mutex::new(None)),
        }
    }

    /// Run the MCP server, reading from stdin and writing to stdout
    pub async fn run(self) -> Result<()> {
        let stdin = tokio::io::stdin();
        let stdout = tokio::io::stdout();
        let mut reader = BufReader::new(stdin);
        let mut writer = BufWriter::new(stdout);

        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) => break, // EOF
                Ok(_) => {
                    if let Some(response) = self.handle_request(&line).await? {
                        writer.write_all(response.as_bytes()).await?;
                        writer.write_all(b"\n").await?;
                        writer.flush().await?;
                    }
                }
                Err(e) => {
                    eprintln!("Error reading from stdin: {}", e);
                    break;
                }
            }
        }
        Ok(())
    }

    /// Handle a single MCP request
    async fn handle_request(&self, request_line: &str) -> Result<Option<String>> {
        let request: Value = serde_json::from_str(request_line.trim())?;
        
        match request.get("method").and_then(|m| m.as_str()) {
            Some("initialize") => Ok(Some(self.handle_initialize(&request).await?)),
            Some("tools/list") => Ok(Some(self.handle_tools_list(&request).await?)),
            Some("tools/call") => Ok(Some(self.handle_tools_call(&request).await?)),
            Some("notifications/initialized") => Ok(None), // No response needed
            Some(method) => {
                let error_response = json!({
                    "jsonrpc": "2.0",
                    "id": request.get("id"),
                    "error": {
                        "code": -32601,
                        "message": format!("Method not found: {}", method)
                    }
                });
                Ok(Some(error_response.to_string()))
            }
            None => {
                let error_response = json!({
                    "jsonrpc": "2.0",
                    "id": request.get("id"),
                    "error": {
                        "code": -32600,
                        "message": "Invalid request: missing method"
                    }
                });
                Ok(Some(error_response.to_string()))
            }
        }
    }

    /// Handle the initialize request
    async fn handle_initialize(&self, request: &Value) -> Result<String> {
        let response = json!({
            "jsonrpc": "2.0",
            "id": request.get("id"),
            "result": {
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {}
                },
                "serverInfo": {
                    "name": "rustdoc-json-mcp",
                    "version": "0.1.0"
                }
            }
        });
        Ok(response.to_string())
    }

    /// Handle the tools/list request
    async fn handle_tools_list(&self, request: &Value) -> Result<String> {
        let tools = vec![
            json!({
                "name": "set_project",
                "description": "Set the cargo project root by pointing to Cargo.toml",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "manifest_path": {
                            "type": "string",
                            "description": "Path to the Cargo.toml file"
                        }
                    },
                    "required": ["manifest_path"]
                }
            }),
            json!({
                "name": "list_crates",
                "description": "List all available crates in the project",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "rebuild": {
                            "type": "boolean",
                            "description": "Rebuild documentation before listing (default: false)",
                            "default": false
                        }
                    },
                    "additionalProperties": false
                }
            }),
            json!({
                "name": "crate_info",
                "description": "Get basic information about a crate",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "crate": {
                            "type": "string",
                            "description": "Name of the crate (optional, defaults to project root)"
                        },
                        "rebuild": {
                            "type": "boolean",
                            "description": "Rebuild documentation before querying (default: false)",
                            "default": false
                        }
                    },
                    "additionalProperties": false
                }
            }),
            json!({
                "name": "list_items_by_kind",
                "description": "List all items of a specific kind (e.g., 'function', 'struct', 'trait')",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "kind": {
                            "type": "string",
                            "description": "The kind of items to list"
                        },
                        "crate": {
                            "type": "string",
                            "description": "Name of the crate (optional, defaults to project root)"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum number of items to return (default: 50)",
                            "default": 50
                        },
                        "rebuild": {
                            "type": "boolean",
                            "description": "Rebuild documentation before querying (default: false)",
                            "default": false
                        }
                    },
                    "required": ["kind"]
                }
            }),
            json!({
                "name": "search_items",
                "description": "Search for items by name",
                "inputSchema": {
                    "type": "object", 
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Search query (case-insensitive substring match)"
                        },
                        "crate": {
                            "type": "string",
                            "description": "Name of the crate (optional, defaults to project root)"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum number of results to return (default: 20)",
                            "default": 20
                        },
                        "rebuild": {
                            "type": "boolean",
                            "description": "Rebuild documentation before querying (default: false)",
                            "default": false
                        }
                    },
                    "required": ["query"]
                }
            }),
            json!({
                "name": "get_item_details",
                "description": "Get detailed information about a specific item by ID",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "id": {
                            "type": "string",
                            "description": "The ID of the item to retrieve"
                        },
                        "crate": {
                            "type": "string",
                            "description": "Name of the crate (optional, defaults to project root)"
                        },
                        "rebuild": {
                            "type": "boolean",
                            "description": "Rebuild documentation before querying (default: false)",
                            "default": false
                        }
                    },
                    "required": ["id"]
                }
            }),
            json!({
                "name": "kind_statistics",
                "description": "Get statistics about item kinds in a crate",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "crate": {
                            "type": "string",
                            "description": "Name of the crate (optional, defaults to project root)"
                        },
                        "rebuild": {
                            "type": "boolean",
                            "description": "Rebuild documentation before querying (default: false)",
                            "default": false
                        }
                    },
                    "additionalProperties": false
                }
            })
        ];

        let response = json!({
            "jsonrpc": "2.0",
            "id": request.get("id"),
            "result": {
                "tools": tools
            }
        });
        Ok(response.to_string())
    }

    /// Handle the tools/call request  
    async fn handle_tools_call(&self, request: &Value) -> Result<String> {
        let params = request.get("params").ok_or_else(|| anyhow!("Missing params"))?;
        let name = params.get("name").and_then(|n| n.as_str())
            .ok_or_else(|| anyhow!("Missing tool name"))?;
        let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

        let result = match name {
            "set_project" => self.handle_set_project(arguments).await,
            "list_crates" => self.handle_list_crates(arguments).await,
            "crate_info" => self.handle_crate_info(arguments).await,
            "list_items_by_kind" => self.handle_list_items_by_kind(arguments).await,
            "search_items" => self.handle_search_items(arguments).await,
            "get_item_details" => self.handle_get_item_details(arguments).await,
            "kind_statistics" => self.handle_kind_statistics(arguments).await,
            _ => Err(anyhow!("Unknown tool: {}", name)),
        };

        let response = match result {
            Ok(content) => json!({
                "jsonrpc": "2.0",
                "id": request.get("id"),
                "result": {
                    "content": [
                        {
                            "type": "text",
                            "text": content
                        }
                    ]
                }
            }),
            Err(e) => json!({
                "jsonrpc": "2.0", 
                "id": request.get("id"),
                "error": {
                    "code": -32603,
                    "message": format!("Tool execution failed: {}", e)
                }
            }),
        };

        Ok(response.to_string())
    }

    /// Set the cargo project
    async fn handle_set_project(&self, arguments: Value) -> Result<String> {
        let manifest_path = arguments.get("manifest_path").and_then(|p| p.as_str())
            .ok_or_else(|| anyhow!("Missing manifest_path argument"))?;

        let project = RustdocProject::from_manifest(manifest_path)?;
        let info = project.project_info();
        
        *self.project.lock().unwrap() = Some(project);
        
        Ok(format!(
            "Project set successfully!\n\
             - Manifest: {}\n\
             - Target dir: {}\n\
             - Available crates: {} ({})",
            info.manifest_path.display(),
            info.target_dir.display(),
            info.available_crates.len(),
            info.available_crates.join(", ")
        ))
    }

    /// List available crates
    async fn handle_list_crates(&self, arguments: Value) -> Result<String> {
        let rebuild = arguments.get("rebuild").and_then(|r| r.as_bool()).unwrap_or(false);
        
        self.with_project(|project| {
            if rebuild {
                project.generate_docs(None, true)?;
            }
            
            let crates = project.available_crates();
            if crates.is_empty() {
                Ok("No crates found. Try running with rebuild: true to generate documentation.".to_string())
            } else {
                let mut result = format!("Available crates ({}):\n\n", crates.len());
                for crate_name in crates {
                    result.push_str(&format!("- {}\n", crate_name));
                }
                Ok(result)
            }
        })
    }

    /// Get crate information
    async fn handle_crate_info(&self, arguments: Value) -> Result<String> {
        let crate_name = arguments.get("crate").and_then(|c| c.as_str());
        let rebuild = arguments.get("rebuild").and_then(|r| r.as_bool()).unwrap_or(false);

        self.with_project(|project| {
            if rebuild {
                project.generate_docs(crate_name, true)?;
            }
            
            let crate_name = crate_name.unwrap_or_else(|| {
                // Default to the first available crate (usually the project root)
                project.available_crates().first().map(|s| s.as_str()).unwrap_or("unknown")
            });
            
            let data = project.load_crate(crate_name)?;
            let info = data.crate_info();
            
            Ok(format!(
                "Crate Information for '{}':\n\
                 - Format version: {}\n\
                 - Crate version: {}\n\
                 - Includes private items: {}\n\
                 - Root ID: {}\n\
                 - Total items: {}\n\
                 - External crates: {}",
                crate_name,
                info.format_version,
                info.crate_version.unwrap_or_else(|| "unknown".to_string()),
                info.includes_private,
                info.root_id.0,
                info.item_count,
                info.external_crates.len()
            ))
        })
    }

    /// List items by kind
    async fn handle_list_items_by_kind(&self, arguments: Value) -> Result<String> {
        let kind = arguments.get("kind").and_then(|k| k.as_str())
            .ok_or_else(|| anyhow!("Missing kind argument"))?;
        let crate_name = arguments.get("crate").and_then(|c| c.as_str());
        let limit = arguments.get("limit").and_then(|l| l.as_u64()).unwrap_or(50) as usize;
        let rebuild = arguments.get("rebuild").and_then(|r| r.as_bool()).unwrap_or(false);

        self.with_project(|project| {
            if rebuild {
                project.generate_docs(crate_name, true)?;
            }
            
            let crate_name = crate_name.unwrap_or_else(|| {
                project.available_crates().first().map(|s| s.as_str()).unwrap_or("unknown")
            });
            
            let data = project.load_crate(crate_name)?;
            let items = data.items_by_kind(kind);
            let limited_items: Vec<_> = items.into_iter().take(limit).collect();
            
            if limited_items.is_empty() {
                return Ok(format!("No items found of kind '{}' in crate '{}'", kind, crate_name));
            }

            let mut result = format!("Found {} items of kind '{}' in crate '{}':\n\n", 
                limited_items.len(), kind, crate_name);
            for (id, item) in limited_items {
                let name = item.name.as_deref().unwrap_or("<unnamed>");
                let visibility = match item.visibility {
                    rustdoc_types::Visibility::Public => "pub",
                    _ => "private",
                };
                result.push_str(&format!("- {} {} (ID: {})\n", visibility, name, id.0));
                if let Some(docs) = &item.docs {
                    if !docs.is_empty() {
                        let preview = docs.lines().next().unwrap_or("");
                        let truncated = if preview.len() > 100 {
                            format!("{}...", &preview[..97])
                        } else {
                            preview.to_string()
                        };
                        result.push_str(&format!("  // {}\n", truncated));
                    }
                }
            }
            Ok(result)
        })
    }

    /// Search items by name
    async fn handle_search_items(&self, arguments: Value) -> Result<String> {
        let query = arguments.get("query").and_then(|q| q.as_str())
            .ok_or_else(|| anyhow!("Missing query argument"))?;
        let crate_name = arguments.get("crate").and_then(|c| c.as_str());
        let limit = arguments.get("limit").and_then(|l| l.as_u64()).unwrap_or(20) as usize;
        let rebuild = arguments.get("rebuild").and_then(|r| r.as_bool()).unwrap_or(false);

        self.with_project(|project| {
            if rebuild {
                project.generate_docs(crate_name, true)?;
            }
            
            let crate_name = crate_name.unwrap_or_else(|| {
                project.available_crates().first().map(|s| s.as_str()).unwrap_or("unknown")
            });
            
            let data = project.load_crate(crate_name)?;
            let items = data.search_items(query);
            let limited_items: Vec<_> = items.into_iter().take(limit).collect();

            if limited_items.is_empty() {
                return Ok(format!("No items found matching '{}' in crate '{}'", query, crate_name));
            }

            let mut result = format!("Found {} items matching '{}' in crate '{}':\n\n", 
                limited_items.len(), query, crate_name);
            for (id, item) in limited_items {
                let name = item.name.as_deref().unwrap_or("<unnamed>");
                let kind = item.inner.kind_name();
                let visibility = match item.visibility {
                    rustdoc_types::Visibility::Public => "pub",
                    _ => "private",
                };
                result.push_str(&format!("- {} {} {} (ID: {})\n", visibility, kind, name, id.0));
            }
            Ok(result)
        })
    }

    /// Get item details
    async fn handle_get_item_details(&self, arguments: Value) -> Result<String> {
        let id_str = arguments.get("id").and_then(|i| i.as_str())
            .ok_or_else(|| anyhow!("Missing id argument"))?;
        let crate_name = arguments.get("crate").and_then(|c| c.as_str());
        let rebuild = arguments.get("rebuild").and_then(|r| r.as_bool()).unwrap_or(false);
        
        let id: u32 = id_str.parse()
            .map_err(|_| anyhow!("Invalid ID format"))?;
        let item_id = rustdoc_types::Id(id);

        self.with_project(|project| {
            if rebuild {
                project.generate_docs(crate_name, true)?;
            }
            
            let crate_name = crate_name.unwrap_or_else(|| {
                project.available_crates().first().map(|s| s.as_str()).unwrap_or("unknown")
            });
            
            let data = project.load_crate(crate_name)?;
            match data.get_item(&item_id) {
                Some(item) => {
                    let mut result = String::new();
                    result.push_str(&format!("Item Details in crate '{}' (ID: {}):\n", crate_name, id));
                    result.push_str(&format!("- Name: {}\n", item.name.as_deref().unwrap_or("<unnamed>")));
                    result.push_str(&format!("- Kind: {}\n", item.inner.kind_name()));
                    result.push_str(&format!("- Visibility: {:?}\n", item.visibility));
                    
                    if let Some(docs) = &item.docs {
                        result.push_str(&format!("- Documentation:\n{}\n", docs));
                    }
                    
                    if let Some(span) = &item.span {
                        result.push_str(&format!("- Source: {}:{}:{}\n", 
                            span.filename.display(), 
                            span.begin.0, 
                            span.begin.1
                        ));
                    }
                    
                    Ok(result)
                }
                None => Err(anyhow!("Item with ID {} not found in crate '{}'", id, crate_name)),
            }
        })
    }

    /// Get kind statistics
    async fn handle_kind_statistics(&self, arguments: Value) -> Result<String> {
        let crate_name = arguments.get("crate").and_then(|c| c.as_str());
        let rebuild = arguments.get("rebuild").and_then(|r| r.as_bool()).unwrap_or(false);

        self.with_project(|project| {
            if rebuild {
                project.generate_docs(crate_name, true)?;
            }
            
            let crate_name = crate_name.unwrap_or_else(|| {
                project.available_crates().first().map(|s| s.as_str()).unwrap_or("unknown")
            });
            
            let data = project.load_crate(crate_name)?;
            let stats = data.kind_statistics();
            let mut result = format!("Item Kind Statistics for crate '{}':\n\n", crate_name);
            
            let mut sorted_stats: Vec<_> = stats.iter().collect();
            sorted_stats.sort_by(|a, b| b.1.cmp(a.1)); // Sort by count descending
            
            for (kind, count) in sorted_stats {
                result.push_str(&format!("- {}: {}\n", kind, count));
            }
            
            Ok(result)
        })
    }

    /// Helper to ensure project is loaded
    fn with_project<F, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(&mut RustdocProject) -> Result<R>,
    {
        let mut project_guard = self.project.lock().unwrap();
        match project_guard.as_mut() {
            Some(project) => f(project),
            None => Err(anyhow!("No project loaded. Use set_project tool first.")),
        }
    }
}
