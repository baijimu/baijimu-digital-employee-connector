//! Workspace choices come from the host-managed public CLI, never its credential file.
use crate::HttpError;
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Workspace {
    pub workspace_id: u64,
    pub name: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Response {
    error_code: String,
    data: Page,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Page {
    list: Vec<Entry>,
    total_pages: u64,
}
#[derive(Deserialize)]
struct Entry {
    id: u64,
    name: String,
}

pub(crate) fn list() -> Result<Vec<Workspace>, HttpError> {
    let mut workspaces = Vec::new();
    let mut page = 1;
    loop {
        let mut command = crate::baijimu_cli::command()
            .map_err(|error| HttpError::internal(error.to_string()))?;
        crate::child_process::isolate_from_connector_environment(&mut command);
        let output = command
            .args([
                "workspace",
                "list",
                "--page",
                &page.to_string(),
                "--page-size",
                "100",
                "--json",
            ])
            .output()
            .map_err(|_| HttpError::internal("无法启动平台 CLI 读取工作区"))?;
        if !output.status.success() {
            return Err(HttpError::new(
                502,
                "平台工作区查询失败，请检查客户端的平台账号授权",
            ));
        }
        let response: Response = serde_json::from_slice(&output.stdout)
            .map_err(|error| HttpError::new(502, format!("平台工作区目录响应无效: {error}")))?;
        if response.error_code != "0" {
            return Err(HttpError::new(502, "平台工作区目录返回错误"));
        }
        for entry in response.data.list {
            if entry.id == 0
                || workspaces
                    .iter()
                    .any(|workspace: &Workspace| workspace.workspace_id == entry.id)
            {
                return Err(HttpError::new(502, "平台工作区目录身份无效或重复"));
            }
            workspaces.push(Workspace {
                workspace_id: entry.id,
                name: entry.name,
            });
        }
        if page >= response.data.total_pages {
            return Ok(workspaces);
        }
        page += 1;
    }
}
