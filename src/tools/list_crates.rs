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
        for (crate_name, description) in project.available_crates_with_descriptions() {
            let workspace_note = if project
                .default_crate_name()
                .is_some_and(|name| *name == crate_name)
            {
                " (aliased as \"crate\")"
            } else if project.workspace_packages().contains(&crate_name) {
                " (workspace)"
            } else {
                ""
            };
            if let Some(description) = description {
                let description = description.replace('\n', " ");
                result.write_fmt(format_args!(
                    "• {crate_name}{workspace_note}\n    {description}\n"
                ));
            } else {
                result.write_fmt(format_args!("• {crate_name}{workspace_note}\n"));
            }
        }

        Ok(result)
    }
}
