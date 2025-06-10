use anyhow::{Result, anyhow};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};

use crate::rustdoc::{ItemKind, RustdocData, RustdocProject};

// Constants
const TOOLS_SCHEMA: &str = include_str!("../tools_schema.json");
const DEFAULT_SEARCH_LIMIT: usize = 20;
const DEFAULT_LIST_LIMIT: usize = 50;

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
            Some("initialize") => Ok(Some(self.handle_initialize(&request).await?)),
            Some("tools/list") => Ok(Some(self.handle_tools_list(&request).await?)),
            Some("tools/call") => Ok(Some(self.handle_tools_call(&request).await?)),
            Some("notifications/initialized") => Ok(None), // No response needed
            Some(method) => Ok(Some(self.create_error_response(
                &request,
                -32601,
                &format!("Method not found: {method}"),
            ))),
            None => Ok(Some(self.create_error_response(
                &request,
                -32600,
                "Invalid request: missing method",
            ))),
        }
    }

    /// Create a JSON-RPC error response
    fn create_error_response(&self, request: &Value, code: i32, message: &str) -> String {
        json!({
            "jsonrpc": "2.0",
            "id": request.get("id"),
            "error": {
                "code": code,
                "message": message
            }
        })
        .to_string()
    }

    /// Create a successful tool response
    fn create_success_response(&self, request: &Value, content: &str) -> String {
        json!({
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
        })
        .to_string()
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
        let tools: Value = serde_json::from_str(TOOLS_SCHEMA)?;
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
        let params = request
            .get("params")
            .ok_or_else(|| anyhow!("Missing params"))?;
        let name = params
            .get("name")
            .and_then(|n| n.as_str())
            .ok_or_else(|| anyhow!("Missing tool name"))?;
        let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

        let result = match name {
            "set_project" => self.handle_set_project(arguments).await,
            "crate_info" => self.handle_crate_info(arguments).await,
            "list_items_by_kind" => self.handle_list_items_by_kind(arguments).await,
            "search_items" => self.handle_search_items(arguments).await,
            "get_item_details" => self.handle_get_item_details(arguments).await,
            "kind_statistics" => self.handle_kind_statistics(arguments).await,
            _ => Err(anyhow!("Unknown tool: {name}")),
        };

        match result {
            Ok(content) => Ok(self.create_success_response(request, &content)),
            Err(e) => Ok(self.create_error_response(
                request,
                -32603,
                &format!("Tool execution failed: {e}"),
            )),
        }
    }

    /// Extract common arguments from request
    fn extract_common_args<'a>(&self, arguments: &'a Value) -> (Option<&'a str>, bool) {
        let crate_name = arguments.get("crate").and_then(|c| c.as_str());
        let rebuild = arguments
            .get("rebuild")
            .and_then(|r| r.as_bool())
            .unwrap_or(false);
        (crate_name, rebuild)
    }

    /// Resolve crate name to first available if not specified
    fn resolve_crate_name<'a>(
        &self,
        project: &'a RustdocProject,
        crate_name: Option<&'a str>,
    ) -> &'a str {
        crate_name.unwrap_or_else(|| {
            project
                .available_crates()
                .first()
                .map(|s| s.as_str())
                .unwrap_or("unknown")
        })
    }

    /// Handle crate loading with rebuild and common error handling
    fn load_crate_with_rebuild(
        &self,
        project: &mut RustdocProject,
        crate_name: Option<&str>,
        rebuild: bool,
    ) -> Result<(String, RustdocData)> {
        if rebuild {
            project.generate_docs(crate_name, true)?;
        }

        let resolved_crate_name = self.resolve_crate_name(project, crate_name);
        let data = project.load_crate(resolved_crate_name)?;
        Ok((resolved_crate_name.to_string(), data))
    }

    /// Set the cargo project
    async fn handle_set_project(&self, arguments: Value) -> Result<String> {
        let manifest_path = arguments
            .get("manifest_path")
            .and_then(|p| p.as_str())
            .ok_or_else(|| anyhow!("Missing manifest_path argument"))?;

        let project = RustdocProject::from_manifest(manifest_path)?;
        let info = project.project_info();

        *self.project.lock().unwrap() = Some(project);

        let mut result = format!(
            "Project set successfully!\n\
             - Manifest: {}\n\
             - Target dir: {}\n\
             - Available crates ({}):\n\n",
            info.manifest_path.display(),
            info.target_dir.display(),
            info.available_crates.len()
        );

        if info.available_crates.is_empty() {
            result.push_str(
                "No crates found. Run tools with rebuild: true to generate documentation.\n",
            );
        } else {
            for crate_name in &info.available_crates {
                result.push_str(&format!("- {crate_name}\n"));
            }
        }

        Ok(result)
    }

    /// Get crate information
    async fn handle_crate_info(&self, arguments: Value) -> Result<String> {
        let (crate_name, rebuild) = self.extract_common_args(&arguments);

        self.with_project(|project| {
            let (resolved_crate_name, data) =
                self.load_crate_with_rebuild(project, crate_name, rebuild)?;
            let info = data.crate_info();

            Ok(format!(
                "Crate Information for '{resolved_crate_name}':\n\
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
        let (crate_name, rebuild) = self.extract_common_args(&arguments);
        let limit = arguments
            .get("limit")
            .and_then(|l| l.as_u64())
            .unwrap_or(DEFAULT_LIST_LIMIT as u64) as usize;

        self.with_project(|project| {
            let (resolved_crate_name, data) =
                self.load_crate_with_rebuild(project, crate_name, rebuild)?;
            let items = data.items_by_kind(kind);
            let limited_items: Vec<_> = items.into_iter().take(limit).collect();

            if limited_items.is_empty() {
                return Ok(format!(
                    "No items found of kind '{kind}' in crate '{resolved_crate_name}'"
                ));
            }

            let mut result = format!(
                "Found {} items of kind '{}' in crate '{}':\n\n",
                limited_items.len(),
                kind,
                resolved_crate_name
            );

            for (id, item) in limited_items {
                let name = item.name.as_deref().unwrap_or("<unnamed>");
                let visibility = match item.visibility {
                    rustdoc_types::Visibility::Public => "pub",
                    _ => "private",
                };
                result.push_str(&format!("- {visibility} {name} (ID: {})\n", id.0));

                // Add documentation preview if available
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
        let (crate_name, rebuild) = self.extract_common_args(&arguments);
        let limit = arguments
            .get("limit")
            .and_then(|l| l.as_u64())
            .unwrap_or(DEFAULT_SEARCH_LIMIT as u64) as usize;

        self.with_project(|project| {
            let (resolved_crate_name, data) =
                self.load_crate_with_rebuild(project, crate_name, rebuild)?;
            let items = data.search_items(query);
            let limited_items: Vec<_> = items.into_iter().take(limit).collect();

            if limited_items.is_empty() {
                return Ok(format!(
                    "No items found matching '{query}' in crate '{resolved_crate_name}'"
                ));
            }

            let mut result = format!(
                "Found {} items matching '{}' in crate '{}':\n\n",
                limited_items.len(),
                query,
                resolved_crate_name
            );

            for (id, item) in limited_items {
                let name = item.name.as_deref().unwrap_or("<unnamed>");
                let kind = item.inner.kind_name();
                let visibility = match item.visibility {
                    rustdoc_types::Visibility::Public => "pub",
                    _ => "private",
                };
                result.push_str(&format!("- {visibility} {kind} {name} (ID: {})\n", id.0));
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
        let (crate_name, rebuild) = self.extract_common_args(&arguments);
        let detailed = arguments
            .get("detailed")
            .and_then(|d| d.as_bool())
            .unwrap_or(false);
        let include_impls = arguments
            .get("include_impls")
            .and_then(|i| i.as_bool())
            .unwrap_or(false);

        let id: u32 = id_str.parse().map_err(|_| anyhow!("Invalid ID format"))?;
        let item_id = rustdoc_types::Id(id);

        self.with_project(|project| {
            let (resolved_crate_name, data) =
                self.load_crate_with_rebuild(project, crate_name, rebuild)?;

            match data.get_item(&item_id) {
                Some(item) => {
                    let mut result = String::new();
                    result.push_str(&format!(
                        "Item Details in crate '{resolved_crate_name}' (ID: {id}):\n"
                    ));
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

                    // Add detailed information based on item type
                    if detailed {
                        match &item.inner {
                            rustdoc_types::ItemEnum::Struct(struct_item) => {
                                result.push_str(&self.format_struct_details(&data, struct_item));
                            }
                            rustdoc_types::ItemEnum::Enum(enum_item) => {
                                result.push_str(&self.format_enum_details(&data, enum_item));
                            }
                            rustdoc_types::ItemEnum::Trait(trait_item) => {
                                result.push_str(&self.format_trait_details(&data, trait_item));
                            }
                            _ => {
                                result.push_str("\n--- No additional detailed information available for this item type ---\n");
                            }
                        }
                    }

                    // Add implementation information
                    if include_impls {
                        result.push_str(&self.format_impl_details(&data, &item_id));
                    }

                    Ok(result)
                }
                None => Err(anyhow!(
                    "Item with ID {id} not found in crate '{resolved_crate_name}'"
                )),
            }
        })
    }

    /// Get kind statistics
    async fn handle_kind_statistics(&self, arguments: Value) -> Result<String> {
        let (crate_name, rebuild) = self.extract_common_args(&arguments);

        self.with_project(|project| {
            let (resolved_crate_name, data) =
                self.load_crate_with_rebuild(project, crate_name, rebuild)?;
            let stats = data.kind_statistics();
            let mut result = format!("Item Kind Statistics for crate '{resolved_crate_name}':\n\n");

            let mut sorted_stats: Vec<_> = stats.iter().collect();
            sorted_stats.sort_by(|a, b| b.1.cmp(a.1)); // Sort by count descending

            for (kind, count) in sorted_stats {
                result.push_str(&format!("- {kind}: {count}\n"));
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
    
    /// Format detailed information for struct items
    fn format_struct_details(&self, data: &RustdocData, struct_item: &rustdoc_types::Struct) -> String {
        let mut result = String::new();
        result.push_str("\n--- Detailed Struct Information ---\n");

        match &struct_item.kind {
            rustdoc_types::StructKind::Unit => {
                result.push_str("- Type: Unit struct\n");
            }
            rustdoc_types::StructKind::Tuple(field_ids) => {
                result.push_str("- Type: Tuple struct\n");
                result.push_str(&format!("- Fields ({}):\n", field_ids.len()));

                let fields = data.resolve_struct_fields(field_ids);
                for (idx, field_item) in fields {
                    result.push_str(&format!(
                        "  {}: {}\n",
                        idx,
                        field_item.name.as_deref().unwrap_or("<unnamed>")
                    ));
                    if let Some(field_docs) = &field_item.docs {
                        if !field_docs.is_empty() {
                            let preview = field_docs.lines().next().unwrap_or("");
                            result.push_str(&format!("    // {preview}\n"));
                        }
                    }
                }
            }
            rustdoc_types::StructKind::Plain { fields, .. } => {
                result.push_str("- Type: Named struct\n");
                result.push_str(&format!("- Fields ({}):\n", fields.len()));

                let field_items = data.resolve_named_struct_fields(fields);
                for (field_id, field_item) in field_items {
                    result.push_str(&format!(
                        "  {}: {} (ID: {})\n",
                        field_item.name.as_deref().unwrap_or("<unnamed>"),
                        field_item.inner.kind_name(),
                        field_id.0
                    ));
                    if let Some(field_docs) = &field_item.docs {
                        if !field_docs.is_empty() {
                            let preview = field_docs.lines().next().unwrap_or("");
                            result.push_str(&format!("    // {preview}\n"));
                        }
                    }
                }
            }
        }

        result
    }

    /// Format detailed information for enum items
    fn format_enum_details(&self, data: &RustdocData, enum_item: &rustdoc_types::Enum) -> String {
        let mut result = String::new();
        result.push_str("\n--- Detailed Enum Information ---\n");
        result.push_str(&format!("- Variants ({}):\n", enum_item.variants.len()));

        let variants = data.resolve_enum_variants(&enum_item.variants);
        for (variant_id, variant_item) in variants {
            result.push_str(&format!(
                "  {}: {} (ID: {})\n",
                variant_item.name.as_deref().unwrap_or("<unnamed>"),
                variant_item.inner.kind_name(),
                variant_id.0
            ));
            if let Some(variant_docs) = &variant_item.docs {
                if !variant_docs.is_empty() {
                    let preview = variant_docs.lines().next().unwrap_or("");
                    result.push_str(&format!("    // {preview}\n"));
                }
            }
        }

        result
    }

    /// Format detailed information for trait items
    fn format_trait_details(&self, data: &RustdocData, trait_item: &rustdoc_types::Trait) -> String {
        let mut result = String::new();
        result.push_str("\n--- Detailed Trait Information ---\n");
        if !trait_item.items.is_empty() {
            result.push_str(&format!("- Associated items ({}):\n", trait_item.items.len()));
            let assoc_items = data.resolve_trait_items(&trait_item.items);
            for (assoc_id, assoc_item) in assoc_items {
                result.push_str(&format!(
                    "  {}: {} (ID: {})\n",
                    assoc_item.name.as_deref().unwrap_or("<unnamed>"),
                    assoc_item.inner.kind_name(),
                    assoc_id.0
                ));
            }
        }

        result
    }

    /// Format implementation information for an item
    fn format_impl_details(&self, data: &RustdocData, item_id: &rustdoc_types::Id) -> String {
        let mut result = String::new();
        result.push_str("\n--- Implementation Information ---\n");
        let impls = data.find_impls_for_type(item_id);

        if impls.is_empty() {
            result.push_str("- No implementations found for this type\n");
        } else {
            result.push_str(&format!("- Found {} implementation(s):\n", impls.len()));

            for (impl_id, impl_item) in impls {
                if let rustdoc_types::ItemEnum::Impl(impl_data) = &impl_item.inner {
                    result.push_str(&format!("  Impl block (ID: {}):\n", impl_id.0));
                    if let Some(_trait_ref) = &impl_data.trait_ {
                        result.push_str("    Type: Trait implementation\n");
                        // Note: We could add more trait resolution here
                    } else {
                        result.push_str("    Type: Inherent implementation\n");
                    }

                    if !impl_data.items.is_empty() {
                        result.push_str(&format!("    Methods ({}):\n", impl_data.items.len()));
                        let methods = data.resolve_impl_methods(impl_data);
                        for (method_id, method_item) in methods {
                            let visibility = match method_item.visibility {
                                rustdoc_types::Visibility::Public => "pub",
                                _ => "private",
                            };
                            result.push_str(&format!(
                                "      {} {}: {} (ID: {})\n",
                                visibility,
                                method_item.name.as_deref().unwrap_or("<unnamed>"),
                                method_item.inner.kind_name(),
                                method_id.0
                            ));
                        }
                    }
                }
            }
        }

        result
    }

}
