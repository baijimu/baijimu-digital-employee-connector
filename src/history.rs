use crate::HttpError;
use serde_json::{json, Value};

// The app-server's paginated storage format is not a supported execution
// contract yet. Select the working format at creation, independently of the
// installed CLI's default. Existing threads are never converted here.
pub(crate) fn configure_new_thread(params: &mut Value) -> Result<(), HttpError> {
    match params.get("historyMode") {
        None | Some(Value::Null) => params["historyMode"] = json!("legacy"),
        Some(mode) if mode == "legacy" => {}
        _ => {
            return Err(HttpError::coded(
                400,
                "数字员工会话当前仅支持 legacy 历史存储模式",
                "CODEX_HISTORY_MODE_UNSUPPORTED",
                json!({}),
            ));
        }
    }
    Ok(())
}

pub(crate) fn list_turns(
    thread_id: &str,
    params: Value,
    request: impl FnOnce(Value) -> Result<Value, HttpError>,
) -> Result<Value, HttpError> {
    let first_page = params.get("cursor").is_none_or(Value::is_null);
    match request(params) {
        Ok(result) => Ok(json!({"result": result})),
        Err(error) if first_page && is_unmaterialized_thread(&error, thread_id) => {
            // Codex represents the normal, pre-first-message state as this
            // specific InvalidRequest. It has no persisted turns to page.
            Ok(json!({"result": {"data": [], "nextCursor": null, "backwardsCursor": null}}))
        }
        // Never switch RPCs on failure: doing so hides the source error and
        // cannot make an unsupported storage format readable.
        Err(error) => Err(error),
    }
}

fn is_unmaterialized_thread(error: &HttpError, thread_id: &str) -> bool {
    error.code.as_ref().and_then(Value::as_str) == Some("CODEX_RPC_ERROR")
        && error.data.as_ref().and_then(|v| v.get("rpcCode")) == Some(&json!(-32600))
        && error.message
            == format!("thread {thread_id} is not materialized yet; thread/turns/list is unavailable before first user message")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rpc_error(code: i64, message: &str) -> HttpError {
        HttpError::coded(
            500,
            message,
            "CODEX_RPC_ERROR",
            json!({"rpcCode": code, "rpcData": null}),
        )
    }

    fn unmaterialized() -> HttpError {
        rpc_error(-32600, "thread new-thread is not materialized yet; thread/turns/list is unavailable before first user message")
    }

    #[test]
    fn new_threads_select_supported_storage_without_changing_other_parameters() {
        for initial in [
            json!({"cwd":"/project"}),
            json!({"historyMode":null,"cwd":"/project"}),
            json!({"historyMode":"legacy","cwd":"/project"}),
        ] {
            let mut params = initial;
            configure_new_thread(&mut params).unwrap();
            assert_eq!(params, json!({"historyMode":"legacy","cwd":"/project"}));
        }
        let mut unsupported = json!({"historyMode":"paginated"});
        assert!(configure_new_thread(&mut unsupported).is_err());
        assert_eq!(unsupported["historyMode"], "paginated");
    }

    #[test]
    fn pre_first_message_is_empty_history_after_one_rpc() {
        let result = list_turns("new-thread", json!({"cursor":null}), |_| {
            Err(unmaterialized())
        })
        .unwrap();
        assert_eq!(
            result,
            json!({"result":{"data":[],"nextCursor":null,"backwardsCursor":null}})
        );
    }

    #[test]
    fn other_errors_and_invalid_page_cursors_are_preserved() {
        for (thread_id, params, error) in [
            ("other-thread", json!({}), unmaterialized()),
            (
                "new-thread",
                json!({"cursor":"older-page"}),
                unmaterialized(),
            ),
            (
                "new-thread",
                json!({}),
                rpc_error(-32601, "list_turns is not supported yet"),
            ),
            (
                "new-thread",
                json!({}),
                rpc_error(-32600, "thread not found"),
            ),
            (
                "new-thread",
                json!({}),
                rpc_error(-32603, "storage read failed"),
            ),
        ] {
            let expected = error.message.clone();
            let actual = list_turns(thread_id, params, |_| Err(error)).unwrap_err();
            assert_eq!(actual.message, expected);
        }
    }

    #[test]
    fn persisted_history_preserves_order_and_both_cursors() {
        let page = json!({"data":[{"id":"newest"},{"id":"older"}],"nextCursor":"next","backwardsCursor":"back"});
        let result = list_turns("existing", json!({"cursor":"page","limit":20}), |params| {
            assert_eq!(params["cursor"], "page");
            Ok(page.clone())
        })
        .unwrap();
        assert_eq!(result["result"], page);
    }
}
