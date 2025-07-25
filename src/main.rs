mod doc_ref;
mod filter;
mod format_context;
mod indent;
mod iterators;
mod request;
mod rustdoc;
mod state;
mod tools;
mod traits;
mod verbosity;

use anyhow::Result;
use mcplease::server_info;
use state::RustdocTools;
use tools::Tools;

const INSTRUCTIONS: &str = "Rustdoc documentation explorer for Rust projects.

Use set_working_directory to set the project directory first, then use get_item to explore types, functions, and other items with their source code.";

fn main() -> Result<()> {
    let mut state = RustdocTools::new()?;

    mcplease::run::<Tools, _>(&mut state, server_info!(), Some(INSTRUCTIONS))
}

#[cfg(test)]
mod tests;
