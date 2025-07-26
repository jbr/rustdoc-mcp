use crate::{state::RustdocTools, traits::WriteFmt};
use anyhow::Result;
use clap::Args;
use mcplease::traits::{Tool, WithExamples};
use mcplease::types::Example;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Default, Debug, Serialize, Deserialize, Args, JsonSchema)]
#[serde(rename = "list_crates")]
/// List available crates in the workspace, including dependencies
pub struct ListCrates {
    #[serde(skip)]
    for_schemars: (),
}

impl WithExamples for ListCrates {
    fn examples() -> Vec<Example<Self>> {
        vec![Example {
            description: "listing crates",
            item: Self::default(),
        }]
    }
}

impl Tool<RustdocTools> for ListCrates {
    fn execute(self, state: &mut RustdocTools) -> Result<String> {
        let project = state.project_context(None)?;

        let mut result = String::new();
        for crate_info in project.crate_info() {
            let crate_name = crate_info.name();

            let note = if crate_info.is_default_crate() {
                " (workspace-local, aliased as \"crate\")".to_string()
            } else if crate_info.crate_type().is_workspace() {
                " (workspace-local)".to_string()
            } else if let Some(version) = crate_info.version() {
                let dev_dep_note = if crate_info.is_dev_dep() {
                    " (dev-dep)"
                } else {
                    ""
                };

                format!(" {version}{dev_dep_note}")
            } else {
                String::new()
            };
            result.write_fmt(format_args!("• {crate_name}{note}\n"));
            if let Some(description) = crate_info.description() {
                let description = description.replace('\n', " ");
                result.write_fmt(format_args!("    {description}\n"));
            }
        }

        Ok(result)
    }
}
