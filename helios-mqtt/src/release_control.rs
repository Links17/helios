use helios_state::models::DeviceTarget;
use helios_state::{LocalState, SeekRequest, TargetState, UpdateOpts};
use thiserror::Error;

use crate::protocol::{ReleaseServiceControl, ReleaseStatusGetPayload};

#[derive(Debug, Error)]
pub enum ReleaseControlError {
    #[error("unknown release action: {0}")]
    UnknownAction(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseAction {
    Start,
    Stop,
    Restart,
}

impl TryFrom<u8> for ReleaseAction {
    type Error = ReleaseControlError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Start),
            2 => Ok(Self::Stop),
            3 => Ok(Self::Restart),
            _ => Err(ReleaseControlError::UnknownAction(value)),
        }
    }
}

fn set_service_started(target: &mut DeviceTarget, service_name: &str, started: bool) -> bool {
    let mut found = false;
    for app in target.apps.values_mut() {
        for release in app.releases.values_mut() {
            if let Some(service) = release.services.get_mut(service_name) {
                service.started = started;
                found = true;
            }

            for service in release.services.values_mut() {
                if service.container_name.as_deref() == Some(service_name) {
                    service.started = started;
                    found = true;
                }
            }
        }
    }

    found
}

fn build_seek_request(target: DeviceTarget) -> SeekRequest {
    SeekRequest {
        target: TargetState::Local { target },
        opts: UpdateOpts::default(),
    }
}

fn build_requests_for_service(
    local_state: &LocalState,
    service: &ReleaseServiceControl,
) -> Result<Vec<SeekRequest>, ReleaseControlError> {
    let action = ReleaseAction::try_from(service.action)?;
    let base_target: DeviceTarget = local_state.device.clone().into();

    let requests = match action {
        ReleaseAction::Start => {
            let mut target = base_target;
            if set_service_started(&mut target, &service.name, true) {
                vec![build_seek_request(target)]
            } else {
                Vec::new()
            }
        }
        ReleaseAction::Stop => {
            let mut target = base_target;
            if set_service_started(&mut target, &service.name, false) {
                vec![build_seek_request(target)]
            } else {
                Vec::new()
            }
        }
        ReleaseAction::Restart => {
            let mut stop_target = base_target;
            if !set_service_started(&mut stop_target, &service.name, false) {
                Vec::new()
            } else {
                let mut start_target = stop_target.clone();
                let _ = set_service_started(&mut start_target, &service.name, true);
                vec![
                    build_seek_request(stop_target),
                    build_seek_request(start_target),
                ]
            }
        }
    };

    Ok(requests)
}

pub fn seek_requests_from_release_control(
    local_state: &LocalState,
    payload: &ReleaseStatusGetPayload,
) -> Result<Vec<SeekRequest>, ReleaseControlError> {
    if payload.order.name != "releaseControl" {
        return Ok(Vec::new());
    }

    let mut requests = Vec::new();
    let services = payload
        .order
        .value
        .as_ref()
        .map(|value| value.services.as_slice())
        .unwrap_or_default();

    for service in services {
        requests.extend(build_requests_for_service(local_state, service)?);
    }

    Ok(requests)
}

#[cfg(test)]
mod tests {
    use helios_state::models::Device;
    use helios_util::types::Uuid;
    use serde_json::json;

    use super::*;

    fn local_state_from_device(device: Device) -> LocalState {
        LocalState {
            authorized_apps: vec![],
            device,
            status: helios_state::UpdateStatus::Done,
        }
    }

    #[test]
    fn it_maps_restart_to_two_seek_requests() {
        let device: Device = serde_json::from_value(json!({
            "uuid": "my-device",
            "apps": {
                "my-app": {
                    "id": 1,
                    "name": "my-app",
                    "releases": {
                        "my-release": {
                            "installed": true,
                            "services": {
                                "svc": {
                                    "id": 1,
                                    "image": "ubuntu:latest",
                                    "container_name": "svc_my-release",
                                    "started": true,
                                    "config": {}
                                }
                            }
                        }
                    }
                }
            }
        }))
        .unwrap();

        let payload: ReleaseStatusGetPayload = serde_json::from_value(json!({
            "requestId": "r1",
            "timestamp": 1,
            "fleetId": "12",
            "deviceUUID": "dev-1",
            "intent": "order",
            "type": "request",
            "order": {
                "name": "releaseControl",
                "value": {
                    "services": [{ "name": "svc", "action": 3 }]
                }
            }
        }))
        .unwrap();

        let requests =
            seek_requests_from_release_control(&local_state_from_device(device), &payload).unwrap();
        assert_eq!(requests.len(), 2);
    }

    #[test]
    fn it_ignores_non_control_payloads() {
        let payload: ReleaseStatusGetPayload = serde_json::from_value(json!({
            "requestId": "r1",
            "timestamp": 1,
            "fleetId": "12",
            "deviceUUID": "dev-1",
            "intent": "order",
            "order": { "name": "releaseStatus" }
        }))
        .unwrap();

        let local_state = LocalState {
            authorized_apps: vec![],
            device: Device::new(Uuid::from("my-device"), None),
            status: helios_state::UpdateStatus::Done,
        };

        let requests = seek_requests_from_release_control(&local_state, &payload).unwrap();
        assert!(requests.is_empty());
    }
}
