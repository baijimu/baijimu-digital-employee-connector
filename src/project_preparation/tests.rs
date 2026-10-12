use super::*;

#[test]
fn configured_directory_must_exist_and_be_absolute() {
    assert!(local_directory("relative").is_err());
    assert!(local_directory("/nonexistent-employee-test-directory").is_err());
    let file = std::env::current_exe().unwrap();
    assert!(local_directory(file.to_str().unwrap()).is_err());
    let directory = std::env::current_dir().unwrap();
    let prepared = local_directory(directory.to_str().unwrap()).unwrap();
    assert_eq!(
        std::path::Path::new(&prepared.directory),
        std::fs::canonicalize(directory).unwrap()
    );
    assert!(prepared.project_id.is_none());
}
