//! An integration test links hover-core as a plain library, where its cfg(test) doesn't
//! reach, as the app's own tests and the agents' do. Its log, and the rest of the data
//! folder, still have to stay out of the user's.

#[test]
fn the_log_goes_to_a_folder_of_the_tests_own() {
    let marker = format!("only in the tests' log {}", hover_core::guid_n());
    hover_core::log::line(&marker);
    let support = hover_core::paths::support();
    if std::env::var_os("HOVER_DATA_DIR").is_none() {
        assert_eq!(support.file_name().unwrap().to_string_lossy(), format!("hover-test-data-{}", std::process::id()), "{}", support.display());
    }
    assert!(std::fs::read_to_string(hover_core::paths::log()).unwrap().contains(&marker));
    // Only read: the real folder is never made or written here.
    if let Some(real) = hover_core::platform::app_data().map(|b| b.join("Hover")) {
        assert!(!support.starts_with(&real), "{} is in {}", support.display(), real.display());
        assert!(!std::fs::read_to_string(real.join("hover.log")).unwrap_or_default().contains(&marker));
    }
    assert!(hover_core::paths::is_test_binary(&std::env::current_exe().unwrap()));
}
