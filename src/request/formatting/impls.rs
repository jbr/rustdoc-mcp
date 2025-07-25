use super::*;
use crate::rustdoc::RUST_CRATES;
use std::cmp::Ordering;

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

impl Request {
    /// Add associated methods for a struct or enum
    pub(super) fn format_associated_methods(
        &self,
        item: DocRef<'_, Item>,
        context: &FormatContext,
    ) -> String {
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
}
