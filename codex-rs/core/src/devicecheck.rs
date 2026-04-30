#[cfg(target_os = "macos")]
use std::path::PathBuf;
#[cfg(target_os = "macos")]
use std::process::Command;

use http::HeaderValue;
#[cfg(target_os = "macos")]
use serde::Deserialize;
#[cfg(any(target_os = "macos", test))]
use serde::Serialize;

pub(crate) const X_OAI_ATTESTATION_HEADER: &str = "x-oai-attestation";
#[cfg(target_os = "macos")]
const CODEX_ELECTRON_RESOURCES_PATH_ENV_VAR: &str = "CODEX_ELECTRON_RESOURCES_PATH";
#[cfg(target_os = "macos")]
const PROBE_APP_NAME: &str = "DeviceCheckProbe.app";
#[cfg(target_os = "macos")]
const PROBE_EXECUTABLE_NAME: &str = "DeviceCheckProbe";
#[cfg(target_os = "macos")]
const CLI_PROBE_DIR_NAME: &str = "devicecheck-probe";

#[cfg(target_os = "macos")]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeviceCheckProbeReport {
    supported: bool,
    token_base64: Option<String>,
    error: Option<String>,
    latency_ms: Option<f64>,
}

#[cfg(any(target_os = "macos", test))]
#[derive(Debug, Serialize)]
struct DeviceCheckHeaderPayload<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    token: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    failure_reason: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    failure_detail: Option<&'a str>,
    #[serde(rename = "t", skip_serializing_if = "Option::is_none")]
    latency_ms: Option<f64>,
}

pub(crate) fn macos_devicecheck_header() -> Option<HeaderValue> {
    #[cfg(not(target_os = "macos"))]
    {
        None
    }

    #[cfg(target_os = "macos")]
    {
        HeaderValue::from_str(&macos_devicecheck_payload()).ok()
    }
}

#[cfg(target_os = "macos")]
fn macos_devicecheck_payload() -> String {
    if std::env::consts::ARCH == "x86_64" {
        return failure_payload(
            "unsupported_architecture",
            Some("DeviceCheck is not supported on Intel Macs"),
            /*latency_ms*/ None,
        );
    }

    let Some(probe_app_path) = probe_app_path() else {
        return failure_payload(
            "probe_app_unavailable",
            /*failure_detail*/ None,
            /*latency_ms*/ None,
        );
    };

    let probe_executable = probe_app_path
        .join("Contents")
        .join("MacOS")
        .join(PROBE_EXECUTABLE_NAME);
    let output = match Command::new(&probe_executable).output() {
        Ok(output) => output,
        Err(err) => {
            return failure_payload(
                "probe_launch_failed",
                Some(&err.to_string()),
                /*latency_ms*/ None,
            );
        }
    };
    if !output.status.success() {
        return failure_payload(
            "probe_failed",
            Some(String::from_utf8_lossy(&output.stderr).trim()),
            /*latency_ms*/ None,
        );
    }

    let report: DeviceCheckProbeReport = match serde_json::from_slice(&output.stdout) {
        Ok(report) => report,
        Err(err) => {
            return failure_payload(
                "probe_output_invalid",
                Some(&err.to_string()),
                /*latency_ms*/ None,
            );
        }
    };
    if !report.supported {
        return failure_payload(
            "unsupported_device",
            /*failure_detail*/ None,
            report.latency_ms,
        );
    }
    if let Some(token) = report.token_base64.as_deref() {
        return token_payload(token, report.latency_ms);
    }

    failure_payload(
        "token_generation_failed",
        report.error.as_deref().or(Some("probe returned no token")),
        report.latency_ms,
    )
}

#[cfg(target_os = "macos")]
fn probe_app_path() -> Option<PathBuf> {
    std::env::var_os(CODEX_ELECTRON_RESOURCES_PATH_ENV_VAR)
        .map(PathBuf::from)
        .map(|resources_path| resources_path.join(PROBE_APP_NAME))
        .or_else(cli_probe_app_path)
}

#[cfg(target_os = "macos")]
fn cli_probe_app_path() -> Option<PathBuf> {
    let executable_path = std::env::current_exe().ok()?;
    let executable_dir = executable_path.parent()?;
    let candidate_paths = [
        executable_dir.join(CLI_PROBE_DIR_NAME).join(PROBE_APP_NAME),
        executable_dir
            .parent()?
            .join(CLI_PROBE_DIR_NAME)
            .join(PROBE_APP_NAME),
    ];

    candidate_paths.into_iter().find(|path| path.exists())
}

#[cfg(any(target_os = "macos", test))]
fn token_payload(token: &str, latency_ms: Option<f64>) -> String {
    serde_json::to_string(&DeviceCheckHeaderPayload {
        token: Some(token),
        failure_reason: None,
        failure_detail: None,
        latency_ms,
    })
    .unwrap_or_else(|_| r#"{"failure_reason":"payload_serialization_failed"}"#.to_string())
}

#[cfg(any(target_os = "macos", test))]
fn failure_payload(
    failure_reason: &str,
    failure_detail: Option<&str>,
    latency_ms: Option<f64>,
) -> String {
    serde_json::to_string(&DeviceCheckHeaderPayload {
        token: None,
        failure_reason: Some(failure_reason),
        failure_detail,
        latency_ms,
    })
    .unwrap_or_else(|_| r#"{"failure_reason":"payload_serialization_failed"}"#.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn token_payload_matches_macos_devicecheck_schema() {
        assert_eq!(
            token_payload("token", /*latency_ms*/ Some(12.5)),
            r#"{"token":"token","t":12.5}"#
        );
    }

    #[test]
    fn failure_payload_matches_macos_devicecheck_schema() {
        assert_eq!(
            failure_payload(
                "unsupported_architecture",
                Some("Intel Mac"),
                /*latency_ms*/ Some(12.5),
            ),
            r#"{"failure_reason":"unsupported_architecture","failure_detail":"Intel Mac","t":12.5}"#
        );
    }
}
