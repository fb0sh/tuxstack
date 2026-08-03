//! Mapping for Docker Engine system information.

use bollard::models::{EventActor, EventMessage, SystemInfo, SystemVersion};

use crate::mapping::containers::from_unix_seconds;
use crate::models::{DockerEvent, DockerSystemInfo};

/// Map bollard system info + version into the domain model.
pub fn map_system_info(info: SystemInfo) -> DockerSystemInfo {
    DockerSystemInfo {
        version: String::new(),
        api_version: String::new(),
        min_api_version: String::new(),
        os: info.os_type.clone().unwrap_or_default(),
        arch: info.architecture.clone().unwrap_or_default(),
        kernel_version: info.kernel_version.clone().unwrap_or_default(),
        operating_system: info.operating_system.clone().unwrap_or_default(),
        server_version: info.server_version.clone().unwrap_or_default(),
        docker_root_dir: info.docker_root_dir.clone().unwrap_or_default(),
        total_memory: info.mem_total.map(|m| m.max(0) as u64).unwrap_or(0),
        n_cpus: info.ncpu.map(|n| n.max(0) as u64).unwrap_or(0),
        name: info.name.clone().unwrap_or_default(),
        driver: info.driver.clone().unwrap_or_default(),
        containers: info.containers.map(|c| c.max(0) as u64).unwrap_or(0),
        containers_running: info
            .containers_running
            .map(|c| c.max(0) as u64)
            .unwrap_or(0),
        containers_paused: info
            .containers_paused
            .map(|c| c.max(0) as u64)
            .unwrap_or(0),
        containers_stopped: info
            .containers_stopped
            .map(|c| c.max(0) as u64)
            .unwrap_or(0),
        images: info.images.map(|c| c.max(0) as u64).unwrap_or(0),
    }
}

/// Fill version fields from the `SystemVersion` response.
pub fn apply_system_version(info: &mut DockerSystemInfo, version: SystemVersion) {
    if let Some(v) = version.version {
        info.version = v;
    }
    if let Some(v) = version.api_version {
        info.api_version = v;
    }
    if let Some(v) = version.min_api_version {
        info.min_api_version = v;
    }
}

/// Map a bollard event message into the domain model.
pub fn map_event(event: EventMessage) -> DockerEvent {
    let actor: Option<EventActor> = event.actor;
    DockerEvent {
        event_type: event
            .typ
            .map(|t| t.to_string())
            .unwrap_or_default(),
        action: event.action.unwrap_or_default(),
        actor_id: actor.as_ref().and_then(|a| a.id.clone()),
        actor_attributes: actor
            .map(|a| a.attributes.unwrap_or_default().into_iter().collect())
            .unwrap_or_default(),
        time: event.time.map(from_unix_seconds),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bollard::models::{
        EventMessageTypeEnum, SystemInfo as SI, SystemVersion as SV,
    };

    #[test]
    fn maps_system_info_fields() {
        let info = SI {
            os_type: Some("linux".into()),
            architecture: Some("x86_64".into()),
            kernel_version: Some("6.10.0".into()),
            operating_system: Some("Arch Linux".into()),
            server_version: Some("29.7.1".into()),
            docker_root_dir: Some("/var/lib/docker".into()),
            mem_total: Some(16_000_000_000),
            ncpu: Some(8),
            name: Some("host".into()),
            driver: Some("overlay2".into()),
            containers: Some(5),
            containers_running: Some(2),
            containers_paused: Some(1),
            containers_stopped: Some(2),
            images: Some(10),
            ..Default::default()
        };
        let mapped = map_system_info(info);
        assert_eq!(mapped.os, "linux");
        assert_eq!(mapped.server_version, "29.7.1");
        assert_eq!(mapped.total_memory, 16_000_000_000);
        assert_eq!(mapped.n_cpus, 8);
        assert_eq!(mapped.containers_running, 2);
        assert_eq!(mapped.images, 10);
    }

    #[test]
    fn applies_version_fields() {
        let mut info = map_system_info(SI::default());
        apply_system_version(
            &mut info,
            SV {
                version: Some("29.7.1".into()),
                api_version: Some("1.55".into()),
                min_api_version: Some("1.24".into()),
                ..Default::default()
            },
        );
        assert_eq!(info.version, "29.7.1");
        assert_eq!(info.api_version, "1.55");
        assert_eq!(info.min_api_version, "1.24");
    }

    #[test]
    fn negative_counts_clamp() {
        let info = SI {
            containers_running: Some(-3),
            ..Default::default()
        };
        let mapped = map_system_info(info);
        assert_eq!(mapped.containers_running, 0);
    }

    #[test]
    fn maps_event_message() {
        let event = EventMessage {
            typ: Some(EventMessageTypeEnum::CONTAINER),
            action: Some("start".into()),
            actor: Some(EventActor {
                id: Some("abc123".into()),
                attributes: Some(
                    vec![("name".to_string(), "web".to_string())]
                        .into_iter()
                        .collect(),
                ),
            }),
            time: Some(1_700_000_000),
            ..Default::default()
        };
        let mapped = map_event(event);
        assert_eq!(mapped.event_type, "container");
        assert_eq!(mapped.action, "start");
        assert_eq!(mapped.actor_id.as_deref(), Some("abc123"));
        assert_eq!(mapped.time.unwrap().timestamp(), 1_700_000_000);
    }
}
