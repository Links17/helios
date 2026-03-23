use std::time::{Duration, SystemTime, UNIX_EPOCH};

use thiserror::Error;
use tokio::process::Command;
use tokio::time::timeout;

use crate::config::{MqttIdentity, ScriptConfig};
use crate::protocol::{
    ScriptRequestPayload, ScriptResponseOrder, ScriptResponsePayload, ScriptResponseValue,
};

#[derive(Debug, Error)]
pub enum ScriptError {
    #[error("failed to execute script: {0}")]
    Execute(std::io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptExecution {
    pub code: i32,
    pub msg: String,
}

fn now_millis() -> i64 {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_millis(0));
    duration.as_millis() as i64
}

fn truncate_message(mut message: String, max_output_bytes: usize) -> String {
    if message.len() <= max_output_bytes {
        return message;
    }

    message.truncate(max_output_bytes);
    message.push_str("...(truncated)");
    message
}

pub async fn execute_script_command(
    command: &str,
    timeout_duration: Duration,
    max_output_bytes: usize,
) -> Result<ScriptExecution, ScriptError> {
    let mut process = Command::new("/bin/sh");
    process.kill_on_drop(true);
    process.arg("-c").arg(command);

    let execution = timeout(timeout_duration, process.output()).await;

    let result = match execution {
        Ok(Ok(output)) => {
            let code = output.status.code().unwrap_or(1);
            let mut msg = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if msg.is_empty() {
                msg = String::from_utf8_lossy(&output.stderr).trim().to_string();
            }
            if msg.is_empty() {
                msg = if code == 0 {
                    "success".to_string()
                } else {
                    format!("command exited with code {code}")
                };
            }
            ScriptExecution {
                code,
                msg: truncate_message(msg, max_output_bytes),
            }
        }
        Ok(Err(error)) => return Err(ScriptError::Execute(error)),
        Err(_) => ScriptExecution {
            code: 124,
            msg: "script execution timed out".to_string(),
        },
    };

    Ok(result)
}

pub async fn handle_script_request(
    payload: &ScriptRequestPayload,
    identity: &MqttIdentity,
    config: &ScriptConfig,
) -> Result<Option<ScriptResponsePayload>, ScriptError> {
    if !config.enable || payload.r#type != "request" {
        return Ok(None);
    }

    let result = if payload.script.expire_time > 0 && payload.script.expire_time < now_millis() {
        ScriptExecution {
            code: 124,
            msg: "script request expired".to_string(),
        }
    } else {
        execute_script_command(
            &payload.script.cmd,
            config.exec_timeout,
            config.max_output_bytes,
        )
        .await?
    };

    Ok(Some(ScriptResponsePayload {
        request_id: payload.request_id.clone(),
        timestamp: now_millis(),
        intent: "order".to_string(),
        response_type: "response".to_string(),
        fleet_id: identity.fleet_id.clone(),
        device_uuid: identity.device_uuid.clone(),
        order: ScriptResponseOrder {
            name: "script".to_string(),
            value: ScriptResponseValue {
                script_name: payload.script.name.clone(),
                version: payload.script.version.clone(),
                code: result.code,
                msg: result.msg,
            },
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ScriptConfig;
    use serde_json::json;

    #[tokio::test]
    async fn it_handles_non_request_messages() {
        let payload: ScriptRequestPayload = serde_json::from_value(json!({
            "requestId": "req-1",
            "timestamp": 10,
            "fleetId": "12",
            "deviceUUID": "dev-1",
            "intent": "script",
            "type": "response",
            "script": {
                "name": "my-script",
                "version": "1.0.0",
                "type": 1,
                "expireTime": 0,
                "cmd": "echo hi"
            }
        }))
        .unwrap();

        let identity = MqttIdentity {
            fleet_id: "12".to_string(),
            device_uuid: "dev-1".to_string(),
        };
        let response = handle_script_request(&payload, &identity, &ScriptConfig::default())
            .await
            .unwrap();
        assert!(response.is_none());
    }

    #[tokio::test]
    async fn it_executes_script_requests() {
        let payload: ScriptRequestPayload = serde_json::from_value(json!({
            "requestId": "req-1",
            "timestamp": 10,
            "fleetId": "12",
            "deviceUUID": "dev-1",
            "intent": "script",
            "type": "request",
            "script": {
                "name": "my-script",
                "version": "1.0.0",
                "type": 1,
                "expireTime": 0,
                "cmd": "echo hello"
            }
        }))
        .unwrap();

        let identity = MqttIdentity {
            fleet_id: "12".to_string(),
            device_uuid: "dev-1".to_string(),
        };
        let response = handle_script_request(&payload, &identity, &ScriptConfig::default())
            .await
            .unwrap()
            .unwrap();

        assert_eq!(response.order.value.code, 0);
        assert_eq!(response.order.value.msg, "hello");
    }
}
