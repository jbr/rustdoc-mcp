use crate::{
    state::RustdocTools,
    tools::{GetItem, ListCrates, SetWorkingDirectory},
};
use mcplease::traits::Tool;
use std::path::PathBuf;

/// Get the path to our test workspace
fn get_test_workspace_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/test-workspace")
}

/// Create a test state with workspace context
fn create_workspace_test_state() -> RustdocTools {
    let mut state = RustdocTools::new(None)
        .expect("Failed to create state")
        .with_default_session_id("workspace_test");

    SetWorkingDirectory {
        path: get_test_workspace_path().to_string_lossy().to_string(),
    }
    .execute(&mut state)
    .unwrap();

    state
}

#[test]
fn test_workspace_list_crates_with_dependencies() {
    let mut state = create_workspace_test_state();
    
    let result = ListCrates::default().execute(&mut state).unwrap();
    
    // Should include workspace members
    assert!(result.contains("• crate-a (workspace-local"));
    assert!(result.contains("• crate-b (workspace-local"));
    
    // Should include external dependencies from all workspace members
    assert!(result.contains("• serde "));
    assert!(result.contains("• anyhow "));
    assert!(result.contains("• regex "));
    assert!(result.contains("• log "));
    
    // Should include dev dependencies
    assert!(result.contains("• tempfile ") && result.contains("(dev-dep)"));
    assert!(result.contains("• env_logger ") && result.contains("(dev-dep)"));
    
    // Should NOT include workspace-internal dependencies
    assert!(!result.contains("• crate-a") || !result.contains(" 0.1.0")); // crate-a shouldn't be listed as external dep
    
    insta::assert_snapshot!(result);
}

#[test]
fn test_workspace_get_crate_a() {
    let mut state = create_workspace_test_state();
    
    let tool = GetItem {
        name: "crate-a".to_string(),
        ..Default::default()
    };
    
    let result = tool.execute(&mut state).expect("Tool execution failed");
    
    // Should contain crate-a items
    assert!(result.contains("CrateAStruct"));
    assert!(result.contains("process_data"));
    
    insta::assert_snapshot!(result);
}

#[test] 
fn test_workspace_get_crate_b() {
    let mut state = create_workspace_test_state();
    
    let tool = GetItem {
        name: "crate-b".to_string(),
        ..Default::default()
    };
    
    let result = tool.execute(&mut state).expect("Tool execution failed");
    
    // Should contain crate-b items
    assert!(result.contains("CrateBProcessor"));
    
    insta::assert_snapshot!(result);
}

#[test]
fn test_workspace_access_dependency() {
    let mut state = create_workspace_test_state();
    
    // Try to access a dependency that should be available
    let tool = GetItem {
        name: "serde".to_string(),
        ..Default::default()
    };
    
    let result = tool.execute(&mut state).expect("Tool execution failed");
    
    // Should be able to access serde documentation
    assert!(result.contains("serde"));
    
    insta::assert_snapshot!(result);
}