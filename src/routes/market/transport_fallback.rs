use serde_json::{json, Map, Value};

use crate::state::AppState;

use super::{payload_ret_success, upstream_app_request, upstream_h5_request};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PreferredTransport {
    AppFirst,
    H5First,
}

pub(super) async fn request_with_fallback(
    state: &AppState,
    uid: &str,
    api: &str,
    version: &str,
    data: Value,
    preferred: PreferredTransport,
) -> Result<Value, String> {
    let mut attempts = Vec::new();
    let (first_label, second_label) = match preferred {
        PreferredTransport::AppFirst => ("app", "h5"),
        PreferredTransport::H5First => ("h5", "app"),
    };

    let first_result = request_transport(state, uid, api, version, data.clone(), first_label).await;
    let mut first_non_success_payload: Option<Value> = None;

    match first_result {
        Ok(payload) if payload_ret_success(&payload) => {
            attempts.push(attempt_from_payload(first_label, &payload));
            return Ok(with_transport_meta(
                payload,
                first_label,
                first_label,
                attempts,
            ));
        }
        Ok(payload) => {
            attempts.push(attempt_from_payload(first_label, &payload));
            if !should_fallback_on_payload(&payload) {
                return Ok(with_transport_meta(
                    payload,
                    first_label,
                    first_label,
                    attempts,
                ));
            }
            first_non_success_payload = Some(payload);
        }
        Err(error) => {
            attempts.push(attempt_from_error(first_label, &error));
        }
    }

    let second_result = request_transport(state, uid, api, version, data, second_label).await;
    match second_result {
        Ok(payload) => {
            attempts.push(attempt_from_payload(second_label, &payload));
            let chosen = if payload_ret_success(&payload) {
                second_label
            } else if first_non_success_payload.is_some() {
                second_label
            } else {
                first_label
            };
            Ok(with_transport_meta(payload, first_label, chosen, attempts))
        }
        Err(error) => {
            attempts.push(attempt_from_error(second_label, &error));
            if let Some(payload) = first_non_success_payload {
                Ok(with_transport_meta(
                    payload,
                    first_label,
                    first_label,
                    attempts,
                ))
            } else {
                Err(format!(
                    "{first_label} -> {second_label} 请求都失败: {error}"
                ))
            }
        }
    }
}

async fn request_transport(
    state: &AppState,
    uid: &str,
    api: &str,
    version: &str,
    data: Value,
    transport: &str,
) -> Result<Value, String> {
    match transport {
        "app" => upstream_app_request(state, uid, api, version, data).await,
        "h5" => upstream_h5_request(state, uid, api, version, data, vec![]).await,
        _ => Err(format!("unsupported transport: {transport}")),
    }
}

fn should_fallback_on_payload(payload: &Value) -> bool {
    if payload_ret_success(payload) {
        return false;
    }
    true
}

fn with_transport_meta(
    upstream: Value,
    preferred: &str,
    chosen: &str,
    attempts: Vec<Value>,
) -> Value {
    let mut object = upstream.as_object().cloned().unwrap_or_default();
    let mut data = match object.remove("data") {
        Some(Value::Object(map)) => map,
        Some(value) => {
            let mut map = Map::new();
            map.insert("rawData".to_string(), value);
            map
        }
        None => Map::new(),
    };
    data.insert(
        "transportMeta".to_string(),
        json!({
            "preferred": preferred,
            "chosen": chosen,
            "fallbackUsed": preferred != chosen,
            "attempts": attempts,
        }),
    );
    object.insert("data".to_string(), Value::Object(data));
    Value::Object(object)
}

fn attempt_from_payload(transport: &str, payload: &Value) -> Value {
    json!({
        "transport": transport,
        "success": payload_ret_success(payload),
        "ret": payload_ret_text(payload),
    })
}

fn attempt_from_error(transport: &str, error: &str) -> Value {
    json!({
        "transport": transport,
        "success": false,
        "error": error,
    })
}

fn payload_ret_text(payload: &Value) -> String {
    payload
        .get("ret")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(Value::as_str)
        .or_else(|| payload.get("ret").and_then(Value::as_str))
        .unwrap_or_default()
        .to_string()
}
