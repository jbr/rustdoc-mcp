use anyhow::{Result, anyhow};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufWriter};

use crate::rustdoc::{ItemKind, RustdocData};

/// MCP Server for rustdoc JSON data
pub struct RustdocMcpServer {
    rustdoc_data: Option<RustdocData>,
}

impl RustdocMcpServer {
    pub fn new() -> Self {
        Self { rustdoc_data: None }
    }

    /// Run the MCP server, reading from stdin and writing to stdout
    pub async fn run(&self) -> Result<()> {
        let stdin = tokio::io::stdin();
        let stdout = tokio::io::stdout();
        let mut reader = tokio::io::BufReader::new(stdin);
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
                    eprintln!("Error reading from stdin: {e}");
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
            Some("initialize") => Ok(Some(self.handle_initialize(request).await?)),
            Some("tools/list") => Ok(Some(self.handle_tools_list(request).await?)),
            Some("tools/call") => Ok(Some(self.handle_tools_call(request).await?)),
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
    async fn handle_initialize(&self, request: Value) -> Result<String> {
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
    async fn handle_tools_list(&self, request: Value) -> Result<String> {
        let tools = vec![
            json!({
                "name": "load_rustdoc",
                "description": "Load a rustdoc JSON file for querying",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Path to the rustdoc JSON file"
                        }
                    },
                    "required": ["path"]
                }
            }),
            json!({
                "name": "crate_info",
                "description": "Get basic information about the loaded crate",
                "inputSchema": {
                    "type": "object",
                    "properties": {},
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
                        "limit": {
                            "type": "integer",
                            "description": "Maximum number of items to return (default: 50)",
                            "default": 50
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
                        "limit": {
                            "type": "integer",
                            "description": "Maximum number of results to return (default: 20)",
                            "default": 20
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
                        }
                    },
                    "required": ["id"]
                }
            }),
            json!({
                "name": "kind_statistics",
                "description": "Get statistics about item kinds in the crate",
                "inputSchema": {
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
                }
            }),
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
    async fn handle_tools_call(&self, request: Value) -> Result<String> {
        let params = request
            .get("params")
            .ok_or_else(|| anyhow!("Missing params"))?;
        let name = params
            .get("name")
            .and_then(|n| n.as_str())
            .ok_or_else(|| anyhow!("Missing tool name"))?;
        let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

        let result = match name {
            "load_rustdoc" => self.handle_load_rustdoc(arguments).await,
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

    /// Load a rustdoc JSON file
    async fn handle_load_rustdoc(&self, arguments: Value) -> Result<String> {
        let path = arguments
            .get("path")
            .and_then(|p| p.as_str())
            .ok_or_else(|| anyhow!("Missing path argument"))?;

        // Note: In a real implementation, we'd store this in the server state
        // For now, we'll just validate the file can be loaded
        match RustdocData::from_file(path) {
            Ok(_) => Ok(format!("Successfully loaded rustdoc data from: {path}")),
            Err(e) => Err(anyhow!("Failed to load rustdoc data: {}", e)),
        }
    }

    /// Get crate information
    async fn handle_crate_info(&self, _arguments: Value) -> Result<String> {
        self.with_rustdoc_data(|data| {
            let info = data.crate_info();
            Ok(format!(
                "Crate Information:\n\
                 - Format version: {}\n\
                 - Crate version: {}\n\
                 - Includes private items: {}\n\
                 - Root ID: {}\n\
                 - Total items: {}\n\
                 - External crates: {}",
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
        let kind = arguments
            .get("kind")
            .and_then(|k| k.as_str())
            .ok_or_else(|| anyhow!("Missing kind argument"))?;
        let limit = arguments
            .get("limit")
            .and_then(|l| l.as_u64())
            .unwrap_or(50) as usize;

        self.with_rustdoc_data(|data| {
            let items = data.items_by_kind(kind);
            let limited_items: Vec<_> = items.into_iter().take(limit).collect();

            if limited_items.is_empty() {
                return Ok(format!("No items found of kind: {kind}"));
            }

            let mut result = format!(
                "Found {} items of kind '{}':\n\n",
                limited_items.len(),
                kind
            );
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
                        result.push_str(&format!("  // {truncated}\n"));
                    }
                }
            }
            Ok(result)
        })
    }

    /// Search items by name
    async fn handle_search_items(&self, arguments: Value) -> Result<String> {
        let query = arguments
            .get("query")
            .and_then(|q| q.as_str())
            .ok_or_else(|| anyhow!("Missing query argument"))?;
        let limit = arguments
            .get("limit")
            .and_then(|l| l.as_u64())
            .unwrap_or(20) as usize;

        self.with_rustdoc_data(|data| {
            let items = data.search_items(query);
            let limited_items: Vec<_> = items.into_iter().take(limit).collect();

            if limited_items.is_empty() {
                return Ok(format!("No items found matching: {query}"));
            }

            let mut result = format!(
                "Found {} items matching '{}':\n\n",
                limited_items.len(),
                query
            );
            for (id, item) in limited_items {
                let name = item.name.as_deref().unwrap_or("<unnamed>");
                let kind = item.inner.kind_name();
                let visibility = match item.visibility {
                    rustdoc_types::Visibility::Public => "pub",
                    _ => "private",
                };
                result.push_str(&format!(
                    "- {} {} {} (ID: {})\n",
                    visibility, kind, name, id.0
                ));
            }
            Ok(result)
        })
    }

    /// Get item details
    async fn handle_get_item_details(&self, arguments: Value) -> Result<String> {
        let id_str = arguments
            .get("id")
            .and_then(|i| i.as_str())
            .ok_or_else(|| anyhow!("Missing id argument"))?;

        let id = id_str.parse().map_err(|_| anyhow!("Invalid ID format"))?;
        let item_id = rustdoc_types::Id(id);

        self.with_rustdoc_data(|data| match data.get_item(&item_id) {
            Some(item) => {
                let mut result = String::new();
                result.push_str(&format!("Item Details (ID: {id}):\n"));
                result.push_str(&format!(
                    "- Name: {}\n",
                    item.name.as_deref().unwrap_or("<unnamed>")
                ));
                result.push_str(&format!("- Kind: {}\n", item.inner.kind_name()));
                result.push_str(&format!("- Visibility: {:?}\n", item.visibility));

                if let Some(docs) = &item.docs {
                    result.push_str(&format!("- Documentation:\n{docs}\n"));
                }

                if let Some(span) = &item.span {
                    result.push_str(&format!(
                        "- Source: {}:{}:{}\n",
                        span.filename.display(),
                        span.begin.0,
                        span.begin.1
                    ));
                }

                Ok(result)
            }
            None => Err(anyhow!("Item with ID {} not found", id)),
        })
    }

    /// Get kind statistics
    async fn handle_kind_statistics(&self, _arguments: Value) -> Result<String> {
        self.with_rustdoc_data(|data| {
            let stats = data.kind_statistics();
            let mut result = String::from("Item Kind Statistics:\n\n");

            let mut sorted_stats: Vec<_> = stats.iter().collect();
            sorted_stats.sort_by(|a, b| b.1.cmp(a.1)); // Sort by count descending

            for (kind, count) in sorted_stats {
                result.push_str(&format!("- {kind}: {count}\n"));
            }

            Ok(result)
        })
    }

    /// Helper to ensure rustdoc data is loaded
    fn with_rustdoc_data<F, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(&RustdocData) -> Result<R>,
    {
        match &self.rustdoc_data {
            Some(data) => f(data),
            None => Err(anyhow!(
                "No rustdoc data loaded. Use load_rustdoc tool first."
            )),
        }
    }
}
