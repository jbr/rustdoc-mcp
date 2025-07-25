use super::*;
use crate::format_context::FormatContext;
use crate::rustdoc::RUST_CRATES;
use crate::traits::WriteFmt;
use crate::verbosity::Verbosity;
use rustdoc_types::{
    Abi, Constant, Enum, Function, FunctionPointer, GenericArg, GenericArgs, GenericBound,
    GenericParamDef, GenericParamDefKind, Generics, Id, Item, ItemEnum, Path, Span, Static, Struct,
    StructKind, Term, Trait, Type, TypeAlias, Union, VariantKind, Visibility, WherePredicate,
};
use std::{cmp::Ordering, collections::HashMap};

#[derive(Debug, PartialEq, Eq)]
enum TraitCategory {
    CrateLocal,
    Std,
    External,
}

#[derive(Debug)]
struct TraitGroup {
    trait_name_base: String,      // e.g., "From" without generics
    implementations: Vec<String>, // e.g., ["From<&str>", "From<String>"]
    category: TraitCategory,
}

mod r#enum;
mod functions;
mod r#module;
mod r#struct;
mod r#trait;

/// Information about documentation text with truncation details
#[derive(Debug, Clone, Default)]
pub(crate) struct DocInfo {
    /// The truncated documentation text (may be complete if not truncated)
    pub(crate) text: String,
    /// Total number of lines in the original documentation
    pub(crate) total_lines: usize,
    /// Number of lines included in the truncated text
    pub(crate) displayed_lines: usize,
    /// Whether the documentation was truncated
    pub(crate) is_truncated: bool,
}

impl DocInfo {
    /// Get the number of lines that were elided (hidden)
    pub(crate) fn elided_lines(&self) -> usize {
        self.total_lines.saturating_sub(self.displayed_lines)
    }

    /// Format the elided line count for display (e.g., "[+5 lines]")
    pub(crate) fn elided_indicator(&self) -> Option<String> {
        if self.is_truncated {
            Some(format!("[+{} lines elided]", self.elided_lines()))
        } else {
            None
        }
    }
}
impl Request {
    /// Enhanced type formatting for signatures
    fn format_type(&self, type_: &Type) -> String {
        match type_ {
            Type::ResolvedPath(path) => self.format_path(path),
            Type::DynTrait(dyn_trait) => {
                let traits: Vec<String> = dyn_trait
                    .traits
                    .iter()
                    .map(|t| self.format_path(&t.trait_))
                    .collect();
                format!("dyn {}", traits.join(" + "))
            }
            Type::Generic(name) => name.clone(),
            Type::Primitive(prim) => prim.clone(),
            Type::Array { type_, len } => {
                format!("[{}; {}]", self.format_type(type_), len)
            }
            Type::Slice(type_) => format!("[{}]", self.format_type(type_)),
            Type::BorrowedRef {
                lifetime,
                is_mutable,
                type_,
                ..
            } => {
                let mut result = String::from("&");
                if let Some(lt) = lifetime {
                    result.write_fmt(format_args!("{lt} "));
                }
                if *is_mutable {
                    result.push_str("mut ");
                }
                result.push_str(&self.format_type(type_));
                result
            }
            Type::RawPointer { is_mutable, type_ } => {
                format!(
                    "*{} {}",
                    if *is_mutable { "mut" } else { "const" },
                    self.format_type(type_)
                )
            }
            Type::FunctionPointer(fp) => self.format_function_pointer(fp),
            Type::Tuple(types) => self.format_tuple(types),
            Type::ImplTrait(bounds) => {
                format!("impl {}", self.format_generic_bounds(bounds))
            }
            Type::Infer => "_".to_string(),
            Type::QualifiedPath {
                name,
                args,
                self_type,
                trait_,
            } => self.format_qualified_path(name, args.as_deref(), self_type, trait_),
            Type::Pat { .. } => "pattern".to_string(), // Handle pattern types
        }
    }

    fn format_tuple(&self, types: &[Type]) -> String {
        format!(
            "({})",
            types
                .iter()
                .map(|t| self.format_type(t))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }

    fn format_function_pointer(&self, fp: &FunctionPointer) -> String {
        let mut result = String::new();
        if !fp.generic_params.is_empty() {
            result.push_str("for<");
            result.push_str(
                &fp.generic_params
                    .iter()
                    .map(|p| self.format_generic_param(p))
                    .collect::<Vec<_>>()
                    .join(", "),
            );
            result.push_str("> ");
        }
        result.push_str("fn(");
        result.push_str(
            &fp.sig
                .inputs
                .iter()
                .map(|(_, t)| self.format_type(t))
                .collect::<Vec<_>>()
                .join(", "),
        );
        result.push(')');
        if let Some(output) = &fp.sig.output {
            result.push_str(" -> ");
            result.push_str(&self.format_type(output));
        }
        result
    }

    fn format_qualified_path(
        &self,
        name: &String,
        args: Option<&GenericArgs>,
        self_type: &Type,
        trait_: &Option<Path>,
    ) -> String {
        // For Self::AssociatedType, use simpler syntax when possible
        if matches!(self_type, Type::Generic(s) if s == "Self") {
            if let Some(trait_path) = trait_ {
                let trait_str = self.format_path(trait_path);
                if trait_str.is_empty() {
                    // If trait path is empty, just use Self::name
                    let mut result = format!("Self::{name}");
                    if let Some(args) = args {
                        result.push_str(&self.format_generic_args(args));
                    }
                    return result;
                } else {
                    // Use full qualified syntax: <Self as Trait>::name
                    let mut result = format!("<Self as {trait_str}>::{name}");
                    if let Some(args) = args {
                        result.push_str(&self.format_generic_args(args));
                    }
                    return result;
                }
            } else {
                // No trait specified, use Self::name
                let mut result = format!("Self::{name}");
                if let Some(args) = args {
                    result.push_str(&self.format_generic_args(args));
                }
                return result;
            }
        }
        // For other types, use full qualified syntax
        let mut result = format!("<{}", self.format_type(self_type));
        if let Some(trait_path) = trait_ {
            result.push_str(" as ");
            result.push_str(&self.format_path(trait_path));
        }
        result.push_str(">::");
        result.push_str(name);
        if let Some(args) = args {
            result.push_str(&self.format_generic_args(args));
        }
        result
    }

    /// Format an item with automatic recursion tracking
    pub(crate) fn format_item(&self, item: DocRef<'_, Item>, context: &FormatContext) -> String {
        let mut result = String::new();

        // Basic item information
        result.write_fmt(format_args!("Item: {}\n", item.name().unwrap_or("unnamed")));
        result.write_fmt(format_args!("Kind: {:?}\n", item.kind()));
        result.write_fmt(format_args!("Visibility: {:?}\n", item.visibility));

        if let Some(path) = item.path() {
            result.write_fmt(format_args!("Defined at: {path}\n"));
        }

        // Add documentation if available
        if let Some(docs) = self.docs_to_show(item, false, context) {
            result.write_fmt(format_args!("\n{docs}\n\n"));
        };

        // Handle different item types
        let str = match &item.inner {
            ItemEnum::Module(_) => self.format_module(item, context),
            ItemEnum::Struct(struct_data) => {
                self.format_struct(item, item.build_ref(struct_data), context)
            }
            ItemEnum::Enum(enum_data) => self.format_enum(item, item.build_ref(enum_data), context),
            ItemEnum::Trait(trait_data) => {
                self.format_trait(item, item.build_ref(trait_data), context)
            }
            ItemEnum::Function(function_data) => {
                result.push('\n');
                self.format_function(item, item.build_ref(function_data), context)
            }
            ItemEnum::TypeAlias(type_alias_data) => {
                self.format_type_alias(item, item.build_ref(type_alias_data), context)
            }
            ItemEnum::Union(union_data) => {
                self.format_union(item, item.build_ref(union_data), context)
            }
            ItemEnum::Constant { type_, const_ } => {
                self.format_constant(item, type_, const_, context)
            }
            ItemEnum::Static(static_data) => self.format_static(item, static_data, context),

            ItemEnum::Macro(macro_def) => {
                format!("Macro definition:\n\n```rust\n{macro_def}\n```")
            }
            _ => {
                // For any other item, just print its name and kind
                format!(
                    "\n{:?} {}\n",
                    item.kind(),
                    item.name().unwrap_or("<unnamed>")
                )
            }
        };

        result.push_str(&str);

        // Add source code if requested
        if context.include_source()
            && let Some(span) = &item.span
        {
            result.push_str(&self.format_source_code(span));
        }

        result
    }

    /// Format a type alias
    fn format_type_alias(
        &self,
        item: DocRef<'_, Item>,
        type_alias: DocRef<'_, TypeAlias>,
        _context: &FormatContext,
    ) -> String {
        let type_str = self.format_type(&type_alias.type_);
        format!(
            "\n```rust\ntype {} = {type_str};\n```\n",
            item.name().unwrap_or("<unnamed>")
        )
    }

    /// Format a union
    fn format_union(
        &self,
        _item: DocRef<'_, Item>,
        _union: DocRef<'_, Union>,
        _context: &FormatContext,
    ) -> String {
        // TODO: Implement union formatting
        "\n[Union formatting not yet implemented]\n".to_string()
    }

    /// Format a constant
    fn format_constant(
        &self,
        item: DocRef<'_, Item>,
        type_: &Type,
        const_: &Constant,
        _context: &FormatContext,
    ) -> String {
        let name = item.name().unwrap_or("<unnamed>");
        let type_str = self.format_type(type_);
        let mut result = format!("\n```rust\nconst {name}: {type_str}");
        if let Some(value) = &const_.value {
            result.write_fmt(format_args!(" = {value}"));
        }
        result.push_str(";\n```\n");
        result
    }

    /// Format a static
    fn format_static(
        &self,
        item: DocRef<'_, Item>,
        static_item: &Static,
        _context: &FormatContext,
    ) -> String {
        let type_str = self.format_type(&static_item.type_);
        let result = format!(
            "\n```rust\nstatic {}: {type_str} = {};\n```\n",
            item.name().unwrap_or("<unnamed>"),
            &static_item.expr
        );
        result
    }

    /// Add associated methods for a struct or enum
    fn format_associated_methods(&self, item: DocRef<'_, Item>, context: &FormatContext) -> String {
        let mut result = String::new();

        let inherent_methods = item.methods().collect::<Vec<_>>();
        // Show inherent methods first
        if !inherent_methods.is_empty() {
            result.push_str(&self.format_item_list(inherent_methods, "Associated Types", context));
        }

        let trait_impls = item.traits().collect::<Vec<_>>();
        // Show trait implementations with verbosity gating
        if !trait_impls.is_empty() {
            let trait_groups = self.group_traits_by_id(&trait_impls);
            if !trait_groups.is_empty() {
                let formatted_traits =
                    self.format_trait_groups_with_verbosity(trait_groups, context);
                result.write_fmt(format_args!("\nImplements: {formatted_traits}\n"));
            };
        }

        result
    }

    fn format_item_list(
        &self,
        mut items: Vec<DocRef<'_, Item>>,
        title: &str,
        context: &FormatContext,
    ) -> String {
        let mut result = String::new();
        result.write_fmt(format_args!("\n{title}:\n\n"));

        items.sort_by(|a, b| {
            match (&a.span, &b.span) {
                (Some(span_a), Some(span_b)) => {
                    // Primary sort by filename
                    let filename_cmp = span_a.filename.cmp(&span_b.filename);
                    if filename_cmp != Ordering::Equal {
                        filename_cmp
                    } else {
                        // Secondary sort by start line
                        let line_cmp = span_a.begin.0.cmp(&span_b.begin.0);
                        if line_cmp != Ordering::Equal {
                            line_cmp
                        } else {
                            // Tertiary sort by start column
                            span_a.begin.1.cmp(&span_b.begin.1)
                        }
                    }
                }
                (Some(_), None) => Ordering::Less, // Items with spans come before items without
                (None, Some(_)) => Ordering::Greater, // Items with spans come before items without
                (None, None) => {
                    // Both without spans, sort by name (lexicographical)
                    a.name.cmp(&b.name)
                }
            }
        });

        for item in items {
            let visibility = match &item.visibility {
                Visibility::Public => "pub ".to_string(),
                Visibility::Default => "".to_string(),
                Visibility::Crate => "pub(crate) ".to_string(),
                Visibility::Restricted { path, .. } => format!("pub({path}) "),
            };

            let name = item.name.as_deref().unwrap_or("<unnamed>");
            let kind = item.kind();

            // For functions, show the signature inline
            if let ItemEnum::Function(inner) = &item.inner {
                let signature = self.format_function_signature(name, inner);
                result.write_fmt(format_args!("• {visibility}{signature}\n"));
            } else {
                result.write_fmt(format_args!("• {visibility}"));

                match kind {
                    rustdoc_types::ItemKind::AssocConst => result.push_str("const"),
                    rustdoc_types::ItemKind::AssocType => result.push_str("type"),
                    other => result.write_fmt(format_args!("{other:?}")),
                }

                result.write_fmt(format_args!(" {name}\n"));
            }
            // Add brief doc preview
            if let Some(docs) = self.docs_to_show(item, true, context) {
                result.write_fmt(format_args!("{}", Indent::new(&docs, 4)));
            }

            result.push('\n');
        }

        result
    }

    /// Format trait groups with verbosity gating and useful trait prioritization
    fn format_trait_groups_with_verbosity(
        &self,
        trait_groups: Vec<TraitGroup>,
        context: &FormatContext,
    ) -> String {
        // Separate trait groups by category
        let mut crate_local = Vec::new();
        let mut std_groups = Vec::new();
        let mut external = Vec::new();

        for group in trait_groups {
            match group.category {
                TraitCategory::CrateLocal => crate_local.push(group),
                TraitCategory::Std => std_groups.push(group),
                TraitCategory::External => external.push(group),
            }
        }

        // Sort each category (and prioritize useful std traits)
        crate_local.sort_by(|a, b| a.trait_name_base.cmp(&b.trait_name_base));
        external.sort_by(|a, b| a.trait_name_base.cmp(&b.trait_name_base));
        self.sort_std_traits_by_usefulness(&mut std_groups);

        // Apply verbosity filtering and formatting
        let mut result_parts: Vec<String> = Vec::new();
        let mut truncation_info: Vec<String> = Vec::new();

        match context.verbosity() {
            Verbosity::Minimal => {
                // Only show crate-local traits
                for group in &crate_local {
                    result_parts.push(self.format_trait_group(group, false));
                }

                let hidden_count = std_groups.len() + external.len();
                if hidden_count > 0 {
                    truncation_info.push(format!("[+{hidden_count} more traits]"));
                }
            }
            Verbosity::Brief => {
                // Show crate-local and external (full), limited std
                for group in &crate_local {
                    result_parts.push(self.format_trait_group(group, false));
                }
                for group in &external {
                    result_parts.push(self.format_trait_group(group, false));
                }

                const STD_TRAIT_LIMIT: usize = 8;
                if std_groups.len() <= STD_TRAIT_LIMIT {
                    for group in std_groups {
                        result_parts.push(self.format_trait_group(&group, false));
                    }
                } else {
                    for group in std_groups.iter().take(STD_TRAIT_LIMIT) {
                        result_parts.push(self.format_trait_group(group, false));
                    }
                    let hidden_std = std_groups.len() - STD_TRAIT_LIMIT;
                    truncation_info.push(format!("[+{hidden_std} more std traits]"));
                }
            }
            Verbosity::Full => {
                // Show everything
                for group in &crate_local {
                    result_parts.push(self.format_trait_group(group, true));
                }
                for group in &external {
                    result_parts.push(self.format_trait_group(group, true));
                }
                for group in std_groups {
                    result_parts.push(self.format_trait_group(&group, true));
                }
            }
        }

        // Combine results
        let mut formatted = result_parts.join(", ");
        if !truncation_info.is_empty() {
            if !formatted.is_empty() {
                formatted.push_str(", ");
            }
            formatted.push_str(&truncation_info.join(", "));
        }

        formatted
    }

    /// Format a single trait group (either collapsed or expanded)
    fn format_trait_group(&self, group: &TraitGroup, show_all_impls: bool) -> String {
        if show_all_impls || group.implementations.len() == 1 {
            // Show all implementations
            group.implementations.join(", ")
        } else {
            // Collapse multiple implementations
            format!(
                "{}<...> ({} impls)",
                group.trait_name_base,
                group.implementations.len()
            )
        }
    }

    /// Sort std traits by usefulness (useful traits first)
    fn sort_std_traits_by_usefulness(&self, std_groups: &mut [TraitGroup]) {
        let useful_std_traits = [
            "Clone",
            "Copy",
            "Debug",
            "Display",
            "Default",
            "Eq",
            "PartialEq",
            "Ord",
            "PartialOrd",
            "Hash",
            "From",
            "Into",
            "TryFrom",
            "TryInto",
            "AsRef",
            "AsMut",
            "Borrow",
            "BorrowMut",
            "ToOwned",
            "Deref",
            "DerefMut",
            "Index",
            "IndexMut",
            "Iterator",
            "IntoIterator",
            "Read",
            "Write",
            "Seek",
            "BufRead",
            "Send",
            "Sync", // fundamental for concurrency
        ];

        std_groups.sort_by(|a, b| {
            let a_useful = useful_std_traits.contains(&a.trait_name_base.as_str());
            let b_useful = useful_std_traits.contains(&b.trait_name_base.as_str());

            match (a_useful, b_useful) {
                (true, false) => Ordering::Less,                // a comes first
                (false, true) => Ordering::Greater,             // b comes first
                _ => a.trait_name_base.cmp(&b.trait_name_base), // alphabetical within same usefulness
            }
        });
    }

    /// Group trait implementations by trait ID to handle multiple generic variants
    fn group_traits_by_id<'a>(&self, trait_impls: &[DocRef<'a, Item>]) -> Vec<TraitGroup> {
        let mut groups: HashMap<Id, TraitGroup> = HashMap::new();

        for impl_block in trait_impls {
            if let ItemEnum::Impl(impl_item) = &impl_block.inner
                && let Some(trait_path) = &impl_item.trait_
            {
                let trait_name = self.format_path(trait_path);
                let trait_name_base = trait_name
                    .split('<')
                    .next()
                    .unwrap_or(&trait_name)
                    .to_string();

                let trait_ = impl_block.get(&trait_path.id);
                let category = self.categorize_trait_by_id(*impl_block, trait_, &trait_name);

                groups
                    .entry(trait_path.id)
                    .or_insert_with(|| TraitGroup {
                        trait_name_base: trait_name_base.clone(),
                        implementations: Vec::new(),
                        category,
                    })
                    .implementations
                    .push(trait_name);
            }
        }

        groups.into_values().collect()
    }

    /// Categorize a trait based on its ID and the actual item data
    fn categorize_trait_by_id(
        &self,
        item: DocRef<'_, Item>,
        trait_: Option<DocRef<'_, Item>>,
        trait_name: &str,
    ) -> TraitCategory {
        // Look up the actual trait item
        if let Some(trait_item) = trait_ {
            // Check if this trait is from the current crate by examining its crate ID
            if trait_item.crate_id == item.crate_id {
                return TraitCategory::CrateLocal;
            }

            // Check if it's from std by examining the crate name
            if let Some(trait_crate) = item.crate_docs().external_crates.get(&trait_item.crate_id)
                && let Some(normalized) = self.project.normalize_crate_name(&trait_crate.name)
                && RUST_CRATES.contains(&normalized)
            {
                return TraitCategory::Std;
            }

            // Everything else is external
            TraitCategory::External
        } else {
            // Fallback to string-based categorization if we can't find the item
            self.categorize_trait_fallback(trait_name)
        }
    }

    /// Fallback string-based trait categorization (for when ID lookup fails)
    fn categorize_trait_fallback(&self, trait_path_str: &str) -> TraitCategory {
        // Check if it's a known std/core trait by name (since paths are often simplified)
        let known_std_traits = [
            "Any",
            "Send",
            "Sync",
            "Unpin",
            "UnwindSafe",
            "RefUnwindSafe",
            "Freeze",
            "Clone",
            "CloneToUninit",
            "Copy",
            "Debug",
            "Display",
            "Default",
            "Eq",
            "PartialEq",
            "Ord",
            "PartialOrd",
            "Hash",
            "From",
            "Into",
            "TryFrom",
            "TryInto",
            "AsRef",
            "AsMut",
            "Borrow",
            "BorrowMut",
            "ToOwned",
            "ToString",
            "Iterator",
            "IntoIterator",
            "ExactSizeIterator",
            "DoubleEndedIterator",
            "Fn",
            "FnMut",
            "FnOnce",
            "Deref",
            "DerefMut",
            "Index",
            "IndexMut",
            "Add",
            "Sub",
            "Mul",
            "Div",
            "Rem",
            "Not",
            "BitAnd",
            "BitOr",
            "BitXor",
            "Shl",
            "Shr",
            "AddAssign",
            "SubAssign",
            "MulAssign",
            "DivAssign",
            "RemAssign",
            "BitAndAssign",
            "BitOrAssign",
            "BitXorAssign",
            "ShlAssign",
            "ShrAssign",
            "Read",
            "Write",
            "Seek",
            "BufRead",
            "Error",
        ];

        // Check if it's from std/core/alloc by prefix or known name
        let base_trait_name = trait_path_str.split('<').next().unwrap_or(trait_path_str);
        let base = trait_path_str.split("::").next().unwrap_or(trait_path_str);

        let normalized = self.project.normalize_crate_name(base);

        if normalized.is_some_and(|normalized| RUST_CRATES.contains(&normalized))
            || known_std_traits.contains(&trait_path_str)
            || known_std_traits.contains(&base_trait_name)
        {
            {
                return TraitCategory::Std;
            }
        }

        // For unqualified names that aren't std traits, assume they're crate-local
        // But first strip generics to check the base trait name
        if !trait_path_str.contains("::") {
            let base_trait_name = trait_path_str.split('<').next().unwrap_or(trait_path_str);
            if !known_std_traits.contains(&base_trait_name) {
                return TraitCategory::CrateLocal;
            }
        }

        // Check if it's from the current crate by prefix
        if self.project.workspace_packages().iter().any(|x| x == base) {
            return TraitCategory::CrateLocal;
        }

        // Everything else is external
        TraitCategory::External
    }
    /// Get documentation to show for an item, handling verbosity and truncation
    ///
    /// Returns None if no docs should be shown, Some(docs) if docs should be displayed.
    /// The `is_listing` parameter affects truncation behavior - listing items get more
    /// aggressive truncation than primary items.
    fn docs_to_show(
        &self,
        item: DocRef<'_, Item>,
        is_listing: bool,
        context: &FormatContext,
    ) -> Option<String> {
        // Extract docs from item
        let docs = item.docs.as_deref()?;
        if docs.is_empty() {
            return None;
        }

        // Apply truncation based on verbosity and context
        match (context.verbosity(), is_listing) {
            (Verbosity::Minimal, _) => None,
            (_, true) => {
                // For listings, even in Full mode, just show first non-empty line with indicator
                let first_line = docs
                    .lines()
                    .find(|line| !line.trim().is_empty())
                    .map(|line| line.trim().to_string())?;

                let total_lines = self.count_lines(docs);
                if total_lines > 1 {
                    Some(format!("{} [+{} more lines]", first_line, total_lines - 1))
                } else {
                    Some(first_line)
                }
            }
            (Verbosity::Full, _) => Some(docs.to_string()),
            (Verbosity::Brief, _) => {
                // For primary items, use paragraph-aware truncation
                let total_lines = self.count_lines(docs);
                let truncated_text = self.truncate_to_paragraph_or_lines(docs, 16);
                let displayed_lines = self.count_lines(&truncated_text);
                let is_truncated = displayed_lines < total_lines;

                let doc_info = DocInfo {
                    text: truncated_text,
                    total_lines,
                    displayed_lines,
                    is_truncated,
                };

                if doc_info.is_truncated {
                    Some(format!(
                        "{}\n{}",
                        doc_info.text,
                        doc_info.elided_indicator().unwrap_or_default()
                    ))
                } else {
                    Some(doc_info.text)
                }
            }
        }
    }

    /// Count the number of lines in a text string
    fn count_lines(&self, text: &str) -> usize {
        if text.is_empty() {
            0
        } else {
            text.lines().count()
        }
    }

    /// Truncate text to first paragraph or max_lines, whichever comes first
    fn truncate_to_paragraph_or_lines(&self, text: &str, max_lines: usize) -> String {
        // Look for the second occurrence of "\n\n" (second paragraph break)
        if let Some(first_break) = text.find("\n\n") {
            let after_first_break = &text[first_break + 2..];
            if let Some(second_break_offset) = after_first_break.find("\n\n") {
                // Found second paragraph break - truncate there
                let second_break_pos = first_break + 2 + second_break_offset;
                let first_section = &text[..second_break_pos];
                let first_section_lines = self.count_lines(first_section);

                // If first section is within line limit, use it
                if first_section_lines <= max_lines {
                    return first_section.to_string();
                }
            }
        }

        // Fall back to line-based truncation (no second paragraph break found, or too long)
        let lines: Vec<&str> = text.lines().collect();
        let cutoff = max_lines.min(lines.len());
        lines[..cutoff].join("\n")
    }
    /// Format source code (placeholder)
    fn format_source_code(&self, span: &Span) -> String {
        // Resolve the file path - if it's relative, make it relative to the project root
        let file_path = if span.filename.is_absolute() {
            span.filename.clone()
        } else {
            self.project.project_root().join(&span.filename)
        };

        let Ok(file_content) = fs::read_to_string(&file_path) else {
            return String::new();
        };

        let lines: Vec<&str> = file_content.lines().collect();

        // rustdoc spans are 1-indexed
        let start_line = span.begin.0.saturating_sub(1);
        let end_line = span.end.0.saturating_sub(1);

        if start_line >= lines.len() {
            return String::new();
        }

        let end_line = end_line.min(lines.len().saturating_sub(1));

        // Add a few lines of context around the item
        let context_lines = if end_line - start_line < 10 { 1 } else { 3 };
        let context_start = start_line.saturating_sub(context_lines);
        let context_end = (end_line + context_lines).min(lines.len().saturating_sub(1));

        let mut result = String::new();
        result.write_fmt(format_args!("\nSource: {}\n", file_path.display()));
        result.push_str("```rust\n");

        for line in lines[context_start..=context_end].iter() {
            result.write_fmt(format_args!("{line}\n"));
        }

        result.push_str("```\n");

        result
    }
}
