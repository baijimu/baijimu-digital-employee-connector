use crate::{employee, employee_invocation, AppState, HttpError};
use serde_json::{json, Value};

pub(crate) fn invoke_http(
    path: &str,
    bytes: &[u8],
    workspace: u64,
    state: &AppState,
) -> Result<Value, HttpError> {
    let _guard = state
        .runtime_operation
        .read()
        .map_err(|_| HttpError::internal("员工配置锁异常"))?;
    let employee = employee::require(state.client.state_dir())?;
    employee.authorize(workspace)?;
    // This request is now accepted. Revocation must not wait for its Codex RPC.
    drop(_guard);
    let body = if bytes.is_empty() {
        json!({})
    } else {
        serde_json::from_slice(bytes).map_err(|error| HttpError::new(400, error.to_string()))?
    };
    let body = employee_invocation::parameters(path, body, &employee)?;
    if path == "/invoke/employeeProfile" {
        return Ok(employee_invocation::profile(&employee));
    }
    if path == "/invoke/prepareProject" {
        let prepared = crate::project_preparation::local_directory(
            employee
                .working_directory
                .as_deref()
                .ok_or_else(|| HttpError::new(409, "员工工作目录尚未配置"))?,
        )?;
        return serde_json::to_value(prepared)
            .map_err(|error| HttpError::internal(error.to_string()));
    }
    crate::invoke::handle_invoke(path, &body, &state.client)
}
