//! The connector owns its single employee. Remote requests never mutate this file.
use crate::HttpError;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fs, io::Write, path::Path};

const SCHEMA_VERSION: &str = "1.0.0";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Employee {
    pub schema_version: String,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub working_directory: Option<String>,
    pub authorized_workspace_ids: BTreeSet<u64>,
}

impl Employee {
    pub fn registered(name: String, description: String, enabled: bool) -> Result<Self, HttpError> {
        let employee = Self {
            schema_version: SCHEMA_VERSION.into(),
            name,
            description,
            enabled,
            working_directory: None,
            authorized_workspace_ids: BTreeSet::new(),
        };
        employee.validate()?;
        Ok(employee)
    }

    fn validate(&self) -> Result<(), HttpError> {
        let version = semver::Version::parse(&self.schema_version)
            .map_err(|_| HttpError::internal("员工配置版本无效"))?;
        if version.to_string() != SCHEMA_VERSION {
            return Err(HttpError::internal("不支持此员工配置版本"));
        }
        if self.name.trim().is_empty()
            || self.name.len() > 200
            || self.description.len() > 4000
            || self.authorized_workspace_ids.contains(&0)
        {
            return Err(HttpError::new(400, "员工资料或授权配置无效"));
        }
        if self
            .working_directory
            .as_ref()
            .is_some_and(|cwd| !Path::new(cwd).is_absolute())
        {
            return Err(HttpError::new(400, "员工执行配置无效"));
        }
        Ok(())
    }

    pub fn authorize(&self, workspace: u64) -> Result<(), HttpError> {
        if !self.enabled || !self.authorized_workspace_ids.contains(&workspace) {
            return Err(HttpError::coded(
                403,
                "员工已停用或未授权此工作区",
                "EMPLOYEE_ACCESS_DENIED",
                serde_json::json!({}),
            ));
        }
        if self.working_directory.is_none() {
            return Err(HttpError::coded(
                409,
                "请先在本地管理界面完成员工执行配置",
                "EMPLOYEE_NOT_CONFIGURED",
                serde_json::json!({}),
            ));
        }
        Ok(())
    }
}

pub(crate) fn load(directory: &Path) -> Result<Option<Employee>, HttpError> {
    let bytes = match fs::read(directory.join("employee.json")) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(HttpError::internal(format!("读取员工配置失败: {error}"))),
    };
    let employee: Employee = serde_json::from_slice(&bytes)
        .map_err(|error| HttpError::internal(format!("员工配置损坏: {error}")))?;
    employee.validate()?;
    Ok(Some(employee))
}

pub(crate) fn require(directory: &Path) -> Result<Employee, HttpError> {
    load(directory)?.ok_or_else(|| {
        HttpError::coded(
            403,
            "员工尚未在本地注册",
            "EMPLOYEE_NOT_REGISTERED",
            serde_json::json!({}),
        )
    })
}

pub(crate) fn save(directory: &Path, employee: &Employee) -> Result<(), HttpError> {
    employee.validate()?;
    let bytes = serde_json::to_vec_pretty(employee)
        .map_err(|error| HttpError::internal(error.to_string()))?;
    let temporary = directory.join(format!("employee-{:016x}.tmp", rand::random::<u64>()));
    let result = (|| -> std::io::Result<()> {
        fs::create_dir_all(directory)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
            fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temporary)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
        }
        #[cfg(not(unix))]
        {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
        }
        fs::rename(&temporary, directory.join("employee.json"))?;
        #[cfg(unix)]
        fs::File::open(directory)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|error| HttpError::internal(format!("保存员工配置失败: {error}")))
}

#[cfg(test)]
mod tests;
