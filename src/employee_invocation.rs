//! Local directory policy stays local; the platform supplies AgentConfig unchanged.
use crate::{employee::Employee, HttpError};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Deserialize)]
struct Manifest {
    methods: Vec<Method>,
}
#[derive(Deserialize)]
struct Method {
    path: String,
    input_schema: InputSchema,
}
#[derive(Deserialize, Default)]
struct InputSchema {
    #[serde(default)]
    properties: BTreeMap<String, InputSchema>,
    #[serde(rename = "oneOf", default)]
    one_of: Vec<InputSchema>,
}

fn input_schema(path: &str) -> Result<InputSchema, HttpError> {
    let manifest: Manifest = serde_json::from_str(include_str!("../connector.json"))
        .map_err(|error| HttpError::internal(format!("方法合同无效: {error}")))?;
    let mut schema = manifest
        .methods
        .into_iter()
        .find(|method| method.path == path)
        .ok_or_else(|| HttpError::new(404, "不支持此远端调用方法"))?
        .input_schema;
    for branch in &mut schema.one_of {
        schema.properties.append(&mut branch.properties);
    }
    Ok(schema)
}

pub(crate) fn profile(employee: &Employee) -> Value {
    json!({"name":employee.name,"description":employee.description,"runtime":"codex",
        "workingDirectory":employee.working_directory})
}

pub(crate) fn parameters(path: &str, body: Value, employee: &Employee) -> Result<Value, HttpError> {
    let schema = input_schema(path)?;
    let source = body
        .as_object()
        .ok_or_else(|| HttpError::new(400, "调用参数必须为对象"))?;
    let mut body = source.clone();
    for key in source.keys() {
        if !schema.properties.contains_key(key) {
            return Err(HttpError::new(400, format!("远端不能设置参数 {key}")));
        }
    }
    let params = match source.get("params") {
        None => serde_json::Map::new(),
        Some(Value::Object(params)) => params.clone(),
        _ => return Err(HttpError::new(400, "params 必须为对象")),
    };
    if source.contains_key("params") {
        let allowed = schema
            .properties
            .get("params")
            .ok_or_else(|| HttpError::new(400, "此方法不支持 params"))?;
        for key in params.keys() {
            if !allowed.properties.contains_key(key) {
                return Err(HttpError::new(400, format!("远端不能设置参数 {key}")));
            }
        }
    }
    for value in [source.get("cwd"), params.get("cwd")].into_iter().flatten() {
        if value.as_str() != employee.working_directory.as_deref() {
            return Err(HttpError::new(
                409,
                "cwd 与本地员工工作目录不一致，请重新选择员工",
            ));
        }
    }
    if let Some(mode) = params.get("collaborationMode") {
        let mode: CollaborationMode = serde_json::from_value(mode.clone())
            .map_err(|error| HttpError::new(400, format!("AgentConfig 协作参数无效: {error}")))?;
        if mode.mode != "default" {
            return Err(HttpError::new(400, "不支持此协作模式"));
        }
        if source
            .get("model")
            .is_some_and(|value| value.as_str() != Some(mode.settings.model.as_str()))
        {
            return Err(HttpError::new(400, "请求中的 AgentConfig 模型不一致"));
        }
    }
    if path == "/invoke/prepareProject"
        && (body.get("type").and_then(Value::as_str) != Some("LOCAL_DIRECTORY")
            || body.get("directory").and_then(Value::as_str)
                != employee.working_directory.as_deref())
    {
        return Err(HttpError::new(403, "远端只能使用员工在本地配置的工作目录"));
    }
    if matches!(path, "/invoke/startThread" | "/invoke/startTurn") {
        body.insert("cwd".into(), json!(employee.working_directory));
    }
    if path == "/invoke/resumeThread" {
        let mut params = params;
        params.insert("cwd".into(), json!(employee.working_directory));
        body.insert("params".into(), Value::Object(params));
    }
    Ok(Value::Object(body))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CollaborationMode {
    mode: String,
    settings: CollaborationSettings,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CollaborationSettings {
    model: String,
    #[serde(rename = "developer_instructions")]
    _developer_instructions: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn employee() -> Employee {
        let mut employee = Employee::registered("员工".into(), "".into(), true).unwrap();
        employee.working_directory = Some("/configured".into());
        employee
    }
    #[test]
    fn agent_config_is_preserved_and_never_supplied_from_local_state() {
        let body = json!({"threadId":"thread","input":"task","model":"platform-model","params":{"collaborationMode":{"mode":"default","settings":{"model":"platform-model","developer_instructions":"Platform Agent instructions"}}}});
        let result = parameters("/invoke/startTurn", body.clone(), &employee()).unwrap();
        assert_eq!(result["model"], body["model"]);
        assert_eq!(result["params"], body["params"]);
        assert_eq!(result["cwd"], "/configured");
        let start = parameters(
            "/invoke/startThread",
            json!({"params":{"model":"platform-model","cwd":"/configured"}}),
            &employee(),
        )
        .unwrap();
        assert_eq!(start["params"]["model"], "platform-model");
        assert!(start["params"].get("developerInstructions").is_none());
        let serialized = serde_json::to_value(employee()).unwrap();
        assert!(serialized.get("model").is_none());
        assert!(serialized.get("instructions").is_none());
    }
    #[test]
    fn remote_cannot_change_local_settings_or_use_raw_rpc() {
        for body in [
            json!({"cwd":"/other"}),
            json!({"params":{"config":{}}}),
            json!({"params":{"sandbox":"danger-full-access"}}),
            json!({"params":{"approvalPolicy":"never"}}),
        ] {
            assert!(parameters("/invoke/startThread", body, &employee()).is_err());
        }
        assert_eq!(
            parameters("/invoke/request", json!({}), &employee())
                .unwrap_err()
                .status,
            404
        );
        assert!(parameters(
            "/invoke/prepareProject",
            json!({"type":"NEW_LOCAL_DIRECTORY","parent":"/tmp","name":"other"}),
            &employee()
        )
        .is_err());
        assert!(parameters(
            "/invoke/prepareProject",
            json!({"type":"LOCAL_DIRECTORY","directory":"/other"}),
            &employee()
        )
        .is_err());
        parameters(
            "/invoke/prepareProject",
            json!({"type":"LOCAL_DIRECTORY","directory":"/configured"}),
            &employee(),
        )
        .unwrap();
    }
}
