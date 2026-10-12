use crate::{employee, AppState, HttpError};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Identity {
    name: String,
    description: String,
    enabled: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Configuration {
    working_directory: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Authorization {
    authorized_workspace_ids: std::collections::BTreeSet<u64>,
}

fn decode<T: serde::de::DeserializeOwned>(body: &Value) -> Result<T, HttpError> {
    serde_json::from_value(body.clone())
        .map_err(|error| HttpError::new(400, format!("管理请求无效: {error}")))
}

pub(crate) fn handle(
    method: &str,
    path: &str,
    body: &Value,
    state: &AppState,
) -> Option<Result<Value, HttpError>> {
    match (method, path) {
        ("GET", "/management/employee") => Some(read(state)),
        ("GET", "/management/workspaces") => Some(
            crate::employee_workspaces::list().map(|workspaces| json!({"workspaces":workspaces})),
        ),
        (
            "POST",
            "/management/employee/register"
            | "/management/employee/update"
            | "/management/employee/configure"
            | "/management/employee/authorize",
        ) => Some(change(path, body, state)),
        _ => None,
    }
}

fn read(state: &AppState) -> Result<Value, HttpError> {
    Ok(json!({"employee":employee::load(state.client.state_dir())?}))
}

fn change(path: &str, body: &Value, state: &AppState) -> Result<Value, HttpError> {
    let _guard = state
        .runtime_operation
        .write()
        .map_err(|_| HttpError::internal("员工配置锁异常"))?;
    let directory = state.client.state_dir();
    let mut employee = if path == "/management/employee/register" {
        if employee::load(directory)?.is_some() {
            return Err(HttpError::new(409, "员工已注册，请修改现有员工"));
        }
        let identity: Identity = decode(body)?;
        employee::Employee::registered(identity.name, identity.description, identity.enabled)?
    } else {
        employee::require(directory)?
    };
    match path {
        "/management/employee/update" => {
            let identity: Identity = decode(body)?;
            employee.name = identity.name;
            employee.description = identity.description;
            employee.enabled = identity.enabled;
        }
        "/management/employee/configure" => {
            let configuration: Configuration = decode(body)?;
            let prepared =
                crate::project_preparation::local_directory(&configuration.working_directory)?;
            employee.working_directory = Some(prepared.directory);
        }
        "/management/employee/authorize" => {
            let authorization: Authorization = decode(body)?;
            let added: Vec<_> = authorization
                .authorized_workspace_ids
                .difference(&employee.authorized_workspace_ids)
                .copied()
                .collect();
            if !added.is_empty() {
                let available = crate::employee_workspaces::list()?;
                if added.iter().any(|id| {
                    !available
                        .iter()
                        .any(|workspace| workspace.workspace_id == *id)
                }) {
                    return Err(HttpError::new(
                        403,
                        "只能授权本机平台账号当前可访问的工作区",
                    ));
                }
            }
            employee.authorized_workspace_ids = authorization.authorized_workspace_ids;
        }
        _ => (),
    }
    employee::save(directory, &employee)?;
    Ok(json!({"employee":employee}))
}
