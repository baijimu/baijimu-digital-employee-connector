use super::*;

#[test]
fn registration_configuration_and_workspace_grant_are_required() {
    let mut employee = Employee::registered("员工".into(), "".into(), true).unwrap();
    assert_eq!(employee.authorize(12).unwrap_err().status, 403);
    employee.authorized_workspace_ids.insert(12);
    assert_eq!(employee.authorize(12).unwrap_err().status, 409);
    employee.working_directory = Some(std::env::current_dir().unwrap().to_string_lossy().into());
    employee.authorize(12).unwrap();
    assert_eq!(employee.authorize(13).unwrap_err().status, 403);
    employee.enabled = false;
    assert_eq!(employee.authorize(12).unwrap_err().status, 403);
    employee.enabled = true;
    employee.authorized_workspace_ids.clear();
    assert_eq!(employee.authorize(12).unwrap_err().status, 403);
}

#[test]
fn persisted_registration_survives_restart_and_corruption_fails_closed() {
    let directory =
        std::env::temp_dir().join(format!("employee-test-{:016x}", rand::random::<u64>()));
    assert!(load(&directory).unwrap().is_none());
    let mut employee = Employee::registered("员工".into(), "说明".into(), true).unwrap();
    employee.authorized_workspace_ids.insert(12);
    save(&directory, &employee).unwrap();
    assert!(load(&directory)
        .unwrap()
        .unwrap()
        .authorized_workspace_ids
        .contains(&12));
    employee.authorized_workspace_ids.clear();
    save(&directory, &employee).unwrap();
    assert!(load(&directory)
        .unwrap()
        .unwrap()
        .authorized_workspace_ids
        .is_empty());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(directory.join("employee.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    fs::write(directory.join("employee.json"), "{broken").unwrap();
    assert!(load(&directory).is_err());
    fs::write(
        directory.join("employee.json"),
        "\"private-value-in-invalid-file\"",
    )
    .unwrap();
    assert!(!load(&directory)
        .unwrap_err()
        .message
        .contains("private-value"));
    fs::remove_dir_all(directory).unwrap();
}
