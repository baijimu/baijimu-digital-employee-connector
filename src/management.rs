//! Local-only account operations. These paths are never Relay methods.
use crate::{AppState, HttpError};
use serde_json::{json, Value};

pub(crate) fn account(
    method: &str,
    path: &str,
    state: &AppState,
) -> Option<Result<Value, HttpError>> {
    let (rpc, params) = match (method, path) {
        ("GET", "/management/account/read") => ("account/read", json!({"refreshToken":false})),
        ("POST", "/management/account/login") => ("account/login/start", json!({"type":"chatgpt"})),
        ("POST", "/management/account/logout") => ("account/logout", json!({})),
        _ => return None,
    };
    Some(state.client.request(rpc, params, None))
}

pub(crate) fn route(
    method: &str,
    path: &str,
    body: Value,
    state: &AppState,
) -> Result<Value, HttpError> {
    if let Some(response) = account(method, path, state) {
        return response;
    }
    if let Some(response) = crate::employee_management::handle(method, path, &body, state) {
        return response;
    }
    match (method, path) {
        ("GET", "/management/v1/setup/state") | ("POST", "/management/v1/setup/ensure-ready") => {
            Ok(state.client.status())
        }
        ("POST", "/management/v1/projects/checkout") => {
            let _lock = state
                .management_operation
                .lock()
                .map_err(|_| HttpError::internal("project lock poisoned"))?;
            let request = serde_json::from_value(body)
                .map_err(|e| HttpError::new(400, format!("项目检出请求无效：{e}")))?;
            serde_json::to_value(
                crate::project_checkout::prepare(request)
                    .map_err(|e| HttpError::internal(e.to_string()))?,
            )
            .map_err(|e| HttpError::internal(e.to_string()))
        }
        _ => Err(HttpError::coded(
            404,
            "接口不受此 Connector 支持",
            "METHOD_NOT_SUPPORTED",
            json!({"path":path}),
        )),
    }
}
