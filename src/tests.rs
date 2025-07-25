use crate::{
    filter::Filter,
    state::RustdocTools,
    tools::{GetItem, SetWorkingDirectory},
    verbosity::Verbosity,
};
use mcplease::traits::Tool;
use std::path::PathBuf;

/// Get the path to our test crate (fast to build, minimal dependencies)
fn get_test_crate_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/test-crate")
}

/// Create a test state with isolated session
fn create_test_state() -> RustdocTools {
    RustdocTools::new()
        .expect("Failed to create state")
        .with_default_session_id("test")
}

#[test]
fn test_set_working_directory() {
    let test_crate_dir = get_test_crate_path();
    let mut state = create_test_state();

    let tool = SetWorkingDirectory {
        path: test_crate_dir.to_string_lossy().to_string(),
    };

    let result = tool.execute(&mut state).expect("Tool execution failed");

    // Normalize the path for consistent snapshots
    let normalized_result = result.replace(
        &test_crate_dir.to_string_lossy().to_string(),
        "/TEST_CRATE_ROOT",
    );
    insta::assert_snapshot!(normalized_result);
}

#[test]
fn test_get_crate_root() {
    let test_crate_dir = get_test_crate_path();
    let mut state = create_test_state();

    // Set working directory first
    let set_dir_tool = SetWorkingDirectory {
        path: test_crate_dir.to_string_lossy().to_string(),
    };
    set_dir_tool
        .execute(&mut state)
        .expect("Failed to set working directory");

    // Get the crate root
    let tool = GetItem {
        name: "crate".to_string(),
        ..Default::default()
    };

    let result = tool.execute(&mut state).expect("Tool execution failed");

    insta::assert_snapshot!(result);
}

#[test]
fn test_show_docs_vs_hide_docs_comparison() {
    let test_crate_dir = get_test_crate_path();
    let mut state = create_test_state();

    // Set working directory
    let set_dir_tool = SetWorkingDirectory {
        path: test_crate_dir.to_string_lossy().to_string(),
    };
    set_dir_tool
        .execute(&mut state)
        .expect("Failed to set working directory");

    // First, get TestStruct with docs shown (default)
    let tool_with_docs = GetItem {
        name: "crate::TestStruct".to_string(),
        ..Default::default()
    };

    let result_with_docs = tool_with_docs
        .execute(&mut state)
        .expect("Tool execution failed");

    // Then get TestStruct with docs hidden
    let tool_no_docs = GetItem {
        name: "crate::TestStruct".to_string(),
        verbosity: Some(Verbosity::Minimal),
        ..Default::default()
    };

    let result_no_docs = tool_no_docs
        .execute(&mut state)
        .expect("Tool execution failed");

    // Verify the difference
    assert!(result_with_docs.len() > result_no_docs.len());

    // Both should contain the struct signature
    assert!(result_with_docs.contains("struct TestStruct"));
    assert!(result_no_docs.contains("struct TestStruct"));

    println!("=== WITH DOCS ({} chars) ===", result_with_docs.len());
    println!("{result_with_docs}");
    println!("\n=== WITHOUT DOCS ({} chars) ===", result_no_docs.len());
    println!("{result_no_docs}");
}

#[test]
fn test_verbosity_minimal() {
    let test_crate_dir = get_test_crate_path();
    let mut state = create_test_state();

    // Set working directory
    let set_dir_tool = SetWorkingDirectory {
        path: test_crate_dir.to_string_lossy().to_string(),
    };
    set_dir_tool
        .execute(&mut state)
        .expect("Failed to set working directory");

    // Get the crate root with documentation hidden
    let tool = GetItem {
        name: "crate".to_string(),
        verbosity: Some(Verbosity::Minimal),
        ..Default::default()
    };

    let result = tool.execute(&mut state).expect("Tool execution failed");

    // The result should not contain documentation text
    assert!(!result.contains("Documentation:"));

    // But should still contain structure information
    assert!(result.contains("Item: test_crate"));
    assert!(
        result.contains("Structs:") || result.contains("Enums:") || result.contains("Functions:")
    );

    insta::assert_snapshot!(result);
}

#[test]
fn test_fuzzy_matching_tool_execute() {
    let test_crate_dir = get_test_crate_path();
    let mut state = create_test_state();

    // Set working directory
    let set_dir_tool = SetWorkingDirectory {
        path: test_crate_dir.to_string_lossy().to_string(),
    };
    set_dir_tool
        .execute(&mut state)
        .expect("Failed to set working directory");

    // Try to access a trait method with a typo - should find TestTrait methods
    let tool = GetItem {
        name: "crate::TestStruct::test_metod".to_string(), // typo: should suggest "test_method"
        ..Default::default()
    };

    let result = tool.execute(&mut state).expect("Tool execution failed");

    insta::assert_snapshot!(result);
}
#[test]
fn test_fuzzy_matching_trait_methods() {
    let test_crate_dir = get_test_crate_path();
    let mut state = create_test_state();

    // Set working directory
    let set_dir_tool = SetWorkingDirectory {
        path: test_crate_dir.to_string_lossy().to_string(),
    };
    set_dir_tool
        .execute(&mut state)
        .expect("Failed to set working directory");

    // Try to access a trait method that should be available via impl
    // This tests whether we collect trait implementation methods
    let tool = GetItem {
        name: "crate::TestStruct::cute".to_string(), // Should suggest "clone" from Clone trait
        ..Default::default()
    };

    let result = tool.execute(&mut state).expect("Tool execution failed");

    // Should contain suggestions from trait implementations
    assert!(result.contains("Did you mean"));
    // Should suggest trait methods that are actually available
    // TestStruct implements Clone, so "clone" should be suggested for "cute"

    insta::assert_snapshot!(result);
}

#[test]
fn test_get_struct_details() {
    let test_crate_dir = get_test_crate_path();
    let mut state = create_test_state();

    // Set working directory
    let set_dir_tool = SetWorkingDirectory {
        path: test_crate_dir.to_string_lossy().to_string(),
    };
    set_dir_tool
        .execute(&mut state)
        .expect("Failed to set working directory");

    // Get TestStruct details
    let tool = GetItem {
        name: "crate::TestStruct".to_string(),
        ..Default::default()
    };

    let result = tool.execute(&mut state).expect("Tool execution failed");

    insta::assert_snapshot!(result);
}

#[test]
fn test_get_struct_with_source() {
    let test_crate_dir = get_test_crate_path();
    let mut state = create_test_state();

    // Set working directory
    let set_dir_tool = SetWorkingDirectory {
        path: test_crate_dir.to_string_lossy().to_string(),
    };
    set_dir_tool
        .execute(&mut state)
        .expect("Failed to set working directory");

    // Get TestStruct details with source
    let tool = GetItem {
        name: "crate::TestStruct".to_string(),
        include_source: Some(true),
        ..Default::default()
    };

    let result = tool.execute(&mut state).expect("Tool execution failed");

    // Normalize project path in source output
    let normalized_result = result.replace(
        &test_crate_dir.to_string_lossy().to_string(),
        "/TEST_CRATE_ROOT",
    );
    insta::assert_snapshot!(normalized_result);
}

#[test]
fn test_get_function_details() {
    let test_crate_dir = get_test_crate_path();
    let mut state = create_test_state();

    // Set working directory
    let set_dir_tool = SetWorkingDirectory {
        path: test_crate_dir.to_string_lossy().to_string(),
    };
    set_dir_tool
        .execute(&mut state)
        .expect("Failed to set working directory");

    // Get test_function details with source
    let tool = GetItem {
        name: "crate::test_function".to_string(),
        include_source: Some(true),
        ..Default::default()
    };

    let result = tool.execute(&mut state).expect("Tool execution failed");

    // Normalize project path in source output
    let normalized_result = result.replace(
        &test_crate_dir.to_string_lossy().to_string(),
        "/TEST_CRATE_ROOT",
    );
    insta::assert_snapshot!(normalized_result);
}

#[test]
fn test_get_submodule() {
    let test_crate_dir = get_test_crate_path();
    let mut state = create_test_state();

    // Set working directory
    let set_dir_tool = SetWorkingDirectory {
        path: test_crate_dir.to_string_lossy().to_string(),
    };
    set_dir_tool
        .execute(&mut state)
        .expect("Failed to set working directory");

    // Get submodule listing
    let tool = GetItem {
        name: "crate::submodule".to_string(),
        include_source: None,
        ..Default::default()
    };

    let result = tool.execute(&mut state).expect("Tool execution failed");

    insta::assert_snapshot!(result);
}

#[test]
fn test_get_enum_details() {
    let test_crate_dir = get_test_crate_path();
    let mut state = create_test_state();

    // Set working directory
    let set_dir_tool = SetWorkingDirectory {
        path: test_crate_dir.to_string_lossy().to_string(),
    };
    set_dir_tool
        .execute(&mut state)
        .expect("Failed to set working directory");

    // Get TestEnum from submodule
    let tool = GetItem {
        name: "crate::submodule::TestEnum".to_string(),
        ..Default::default()
    };

    let result = tool.execute(&mut state).expect("Tool execution failed");

    insta::assert_snapshot!(result);
}

#[test]
fn test_get_generic_struct() {
    let test_crate_dir = get_test_crate_path();
    let mut state = create_test_state();

    // Set working directory
    let set_dir_tool = SetWorkingDirectory {
        path: test_crate_dir.to_string_lossy().to_string(),
    };
    set_dir_tool
        .execute(&mut state)
        .expect("Failed to set working directory");

    // Get GenericStruct details
    let tool = GetItem {
        name: "crate::GenericStruct".to_string(),
        ..Default::default()
    };

    let result = tool.execute(&mut state).expect("Tool execution failed");

    insta::assert_snapshot!(result);
}

#[test]
fn test_get_generic_function() {
    let test_crate_dir = get_test_crate_path();
    let mut state = create_test_state();

    // Set working directory
    let set_dir_tool = SetWorkingDirectory {
        path: test_crate_dir.to_string_lossy().to_string(),
    };
    set_dir_tool
        .execute(&mut state)
        .expect("Failed to set working directory");

    // Get generic_function details
    let tool = GetItem {
        name: "crate::generic_function".to_string(),
        ..Default::default()
    };

    let result = tool.execute(&mut state).expect("Tool execution failed");

    insta::assert_snapshot!(result);
}

#[test]
fn test_get_constants() {
    let test_crate_dir = get_test_crate_path();
    let mut state = create_test_state();

    // Set working directory
    let set_dir_tool = SetWorkingDirectory {
        path: test_crate_dir.to_string_lossy().to_string(),
    };
    set_dir_tool
        .execute(&mut state)
        .expect("Failed to set working directory");

    // Get constant
    let tool = GetItem {
        name: "crate::TEST_CONSTANT".to_string(),
        ..Default::default()
    };

    let result = tool.execute(&mut state).expect("Tool execution failed");

    insta::assert_snapshot!(result);
}

#[test]
fn test_get_struct_with_private_fields() {
    let test_crate_dir = get_test_crate_path();
    let mut state = create_test_state();

    // Set working directory
    let set_dir_tool = SetWorkingDirectory {
        path: test_crate_dir.to_string_lossy().to_string(),
    };
    set_dir_tool
        .execute(&mut state)
        .expect("Failed to set working directory");

    // Get GenericStruct to see hidden field indicator
    let tool = GetItem {
        name: "crate::GenericStruct".to_string(),
        ..Default::default()
    };

    let result = tool.execute(&mut state).expect("Tool execution failed");

    insta::assert_snapshot!(result);
}

#[test]
fn test_fuzzy_matching_suggestions() {
    let test_crate_dir = get_test_crate_path();
    let mut state = create_test_state();

    // Set working directory
    let set_dir_tool = SetWorkingDirectory {
        path: test_crate_dir.to_string_lossy().to_string(),
    };
    set_dir_tool
        .execute(&mut state)
        .expect("Failed to set working directory");

    // Try to get a non-existent item that should trigger fuzzy suggestions
    let tool = GetItem {
        name: "crate::TestStruct::incrementCount".to_string(), // typo: should be increment_count
        ..Default::default()
    };

    let result = tool.execute(&mut state).expect("Tool execution failed");

    // Should contain suggestions
    assert!(result.contains("Did you mean"));
    assert!(result.contains("increment_count"));

    insta::assert_snapshot!(result);
}
#[test]
fn test_nonexistent_item() {
    let test_crate_dir = get_test_crate_path();
    let mut state = create_test_state();

    // Set working directory
    let set_dir_tool = SetWorkingDirectory {
        path: test_crate_dir.to_string_lossy().to_string(),
    };
    set_dir_tool
        .execute(&mut state)
        .expect("Failed to set working directory");

    // Try to get a nonexistent item
    let tool = GetItem {
        name: "crate::DoesNotExist".to_string(),
        include_source: None,
        ..Default::default()
    };

    let result = tool.execute(&mut state).expect("Tool execution failed");

    insta::assert_snapshot!(result);
}

#[test]
fn test_get_unit_struct() {
    let test_crate_dir = get_test_crate_path();
    let mut state = create_test_state();

    // Set working directory
    let set_dir_tool = SetWorkingDirectory {
        path: test_crate_dir.to_string_lossy().to_string(),
    };
    set_dir_tool
        .execute(&mut state)
        .expect("Failed to set working directory");

    // Get unit struct details
    let tool = GetItem {
        name: "crate::UnitStruct".to_string(),
        ..Default::default()
    };

    let result = tool.execute(&mut state).expect("Tool execution failed");

    insta::assert_snapshot!(result);
}

#[test]
fn test_get_tuple_struct() {
    let test_crate_dir = get_test_crate_path();
    let mut state = create_test_state();

    // Set working directory
    let set_dir_tool = SetWorkingDirectory {
        path: test_crate_dir.to_string_lossy().to_string(),
    };
    set_dir_tool
        .execute(&mut state)
        .expect("Failed to set working directory");

    // Get tuple struct details
    let tool = GetItem {
        name: "crate::TupleStruct".to_string(),
        ..Default::default()
    };

    let result = tool.execute(&mut state).expect("Tool execution failed");

    insta::assert_snapshot!(result);
}

#[test]
fn test_get_generic_enum() {
    let test_crate_dir = get_test_crate_path();
    let mut state = create_test_state();

    // Set working directory
    let set_dir_tool = SetWorkingDirectory {
        path: test_crate_dir.to_string_lossy().to_string(),
    };
    set_dir_tool
        .execute(&mut state)
        .expect("Failed to set working directory");

    // Get generic enum details
    let tool = GetItem {
        name: "crate::GenericEnum".to_string(),
        ..Default::default()
    };

    let result = tool.execute(&mut state).expect("Tool execution failed");

    insta::assert_snapshot!(result);
}

#[test]
fn test_get_trait_details() {
    let test_crate_dir = get_test_crate_path();
    let mut state = create_test_state();

    // Set working directory
    let set_dir_tool = SetWorkingDirectory {
        path: test_crate_dir.to_string_lossy().to_string(),
    };
    set_dir_tool
        .execute(&mut state)
        .expect("Failed to set working directory");

    // Get TestTrait details
    let tool = GetItem {
        name: "crate::TestTrait".to_string(),
        ..Default::default()
    };

    let result = tool.execute(&mut state).expect("Tool execution failed");

    insta::assert_snapshot!(result);
}

#[test]
fn test_recursive_module_listing() {
    let test_crate_dir = get_test_crate_path();
    let mut state = create_test_state();

    // Set working directory
    let set_dir_tool = SetWorkingDirectory {
        path: test_crate_dir.to_string_lossy().to_string(),
    };
    set_dir_tool
        .execute(&mut state)
        .expect("Failed to set working directory");

    // Get recursive listing of the crate root
    let tool = GetItem {
        name: "crate".to_string(),
        recursive: Some(true),
        ..Default::default()
    };

    let result = tool.execute(&mut state).expect("Tool execution failed");

    insta::assert_snapshot!(result);
}

#[test]
fn test_recursive_submodule_listing() {
    let test_crate_dir = get_test_crate_path();
    let mut state = create_test_state();

    // Set working directory
    let set_dir_tool = SetWorkingDirectory {
        path: test_crate_dir.to_string_lossy().to_string(),
    };
    set_dir_tool
        .execute(&mut state)
        .expect("Failed to set working directory");

    // Get recursive listing of a submodule
    let tool = GetItem {
        name: "crate::submodule".to_string(),
        recursive: Some(true),
        ..Default::default()
    };

    let result = tool.execute(&mut state).expect("Tool execution failed");

    insta::assert_snapshot!(result);
}

#[test]
fn test_recursive_filtering() {
    let test_crate_dir = get_test_crate_path();
    let mut state = create_test_state();

    // Set working directory
    let set_dir_tool = SetWorkingDirectory {
        path: test_crate_dir.to_string_lossy().to_string(),
    };
    set_dir_tool
        .execute(&mut state)
        .expect("Failed to set working directory");

    // Get recursive listing with struct filter only
    let tool = GetItem {
        name: "crate".to_string(),
        recursive: Some(true),
        filter: Some(vec![Filter::Struct]),
        ..Default::default()
    };

    let result = tool.execute(&mut state).expect("Tool execution failed");

    insta::assert_snapshot!(result);
}

#[test]
fn test_non_recursive_filtering() {
    let test_crate_dir = get_test_crate_path();
    let mut state = create_test_state();

    // Set working directory
    let set_dir_tool = SetWorkingDirectory {
        path: test_crate_dir.to_string_lossy().to_string(),
    };
    set_dir_tool
        .execute(&mut state)
        .expect("Failed to set working directory");

    // Get non-recursive listing with struct filter
    let tool = GetItem {
        name: "crate".to_string(),
        filter: Some(vec![Filter::Struct]),
        ..Default::default()
    };

    let result = tool.execute(&mut state).expect("Tool execution failed");

    insta::assert_snapshot!(result);
}

#[test]
fn test_recursive_multiple_filters() {
    let test_crate_dir = get_test_crate_path();
    let mut state = create_test_state();

    // Set working directory
    let set_dir_tool = SetWorkingDirectory {
        path: test_crate_dir.to_string_lossy().to_string(),
    };
    set_dir_tool
        .execute(&mut state)
        .expect("Failed to set working directory");

    // Get recursive listing with function and trait filters
    let tool = GetItem {
        name: "crate".to_string(),
        recursive: Some(true),
        filter: Some(vec![Filter::Function, Filter::Trait]),
        ..Default::default()
    };

    let result = tool.execute(&mut state).expect("Tool execution failed");

    insta::assert_snapshot!(result);
}

#[test]
fn test_get_std_vec() {
    let mut state = create_test_state();

    // Get the root of the std crate
    let tool_std_root = GetItem {
        name: "std".to_string(),
        ..Default::default()
    };
    let result_std_root = tool_std_root
        .execute(&mut state)
        .expect("Tool execution failed for std root");
    insta::assert_snapshot!(result_std_root);

    // Get std::collections::HashMap
    let tool_std_collections_hashmap = GetItem {
        name: "std::collections::HashMap".to_string(),
        ..Default::default()
    };
    let result_std_collections_hashmap = tool_std_collections_hashmap
        .execute(&mut state)
        .expect("Tool execution failed for std::collections::HashMap");
    insta::assert_snapshot!(result_std_collections_hashmap);

    // Get std::vec::Vec
    let tool_std_vec_vec = GetItem {
        name: "std::vec::Vec".to_string(),
        ..Default::default()
    };
    let result_std_vec_vec = tool_std_vec_vec
        .execute(&mut state)
        .expect("Tool execution failed for std::vec::Vec");
    insta::assert_snapshot!(result_std_vec_vec);
}
#[test]
fn test_get_item_with_normalized_crate_name() {
    let test_crate_dir = get_test_crate_path();
    let mut state = create_test_state();

    // Set working directory first
    let set_dir_tool = SetWorkingDirectory {
        path: test_crate_dir.to_string_lossy().to_string(),
    };
    set_dir_tool
        .execute(&mut state)
        .expect("Failed to set working directory");

    // Get an item from the test-crate using a hyphen in the name
    let tool = GetItem {
        name: "test-crate::TestStruct".to_string(),
        ..Default::default()
    };

    let result = tool.execute(&mut state).expect("Tool execution failed");

    insta::assert_snapshot!(result);
}
#[test]
fn test_get_complex_trait_details() {
    let test_crate_dir = get_test_crate_path();
    let mut state = create_test_state();

    // Set working directory
    let set_dir_tool = SetWorkingDirectory {
        path: test_crate_dir.to_string_lossy().to_string(),
    };
    set_dir_tool
        .execute(&mut state)
        .expect("Failed to set working directory");

    // Get ComplexTrait details
    let tool = GetItem {
        name: "crate::ComplexTrait".to_string(),
        ..Default::default()
    };

    let result = tool.execute(&mut state).expect("Tool execution failed");

    insta::assert_snapshot!(result);
}

#[test]
fn tools_doesnt_panic() {
    use crate::tools::Tools;
    use mcplease::traits::AsToolsList;
    Tools::tools_list();
}
