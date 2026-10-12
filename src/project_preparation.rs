//! Directory validation for local employee configuration.
use crate::HttpError;
use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreparedProject {
    pub directory: String,
    pub project_id: Option<u64>,
}

fn absolute_directory(value: &str) -> Result<PathBuf, HttpError> {
    let path = Path::new(value);
    if !path.is_absolute()
        || value.len() > 1024
        || value.chars().any(char::is_control)
        || value.trim() != value
    {
        return Err(HttpError::new(400, "必须提供设备上的绝对目录路径"));
    }
    let directory = fs::canonicalize(path)
        .map_err(|error| HttpError::new(400, format!("设备目录不可访问: {error}")))?;
    if !directory.is_dir() {
        return Err(HttpError::new(400, "所选路径不是目录"));
    }
    Ok(directory)
}

pub(crate) fn local_directory(value: &str) -> Result<PreparedProject, HttpError> {
    let directory = absolute_directory(value)?;
    let text = directory
        .to_str()
        .ok_or_else(|| HttpError::new(400, "目录路径不是有效 UTF-8"))?;
    if text.len() > 1024 {
        return Err(HttpError::new(400, "目录路径超过会话长度限制"));
    }
    Ok(PreparedProject {
        directory: text.into(),
        project_id: None,
    })
}

#[cfg(test)]
mod tests;
