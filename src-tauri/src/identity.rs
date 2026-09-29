use serde::Serialize;
use tauri::{Manager, Runtime};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum NativeChannel {
    Dev,
    Beta,
}

impl NativeChannel {
    fn from_identifier(identifier: &str) -> Result<Self, String> {
        match identifier {
            "dev.pixel.workspace.dev" => Ok(Self::Dev),
            "dev.pixel.workspace" => Ok(Self::Beta),
            _ => Err(format!("无法识别应用标识，无法确认版本渠道：{identifier}")),
        }
    }

    fn suffix(self) -> &'static str {
        match self {
            Self::Dev => "dev",
            Self::Beta => "beta",
        }
    }

    fn app_name(self) -> &'static str {
        match self {
            Self::Dev => "Atrio WorkSpace Dev",
            Self::Beta => "Atrio WorkSpace Beta",
        }
    }

    fn version_label(self, version: &str) -> String {
        format!("{version}-{}", self.suffix())
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AppInfo {
    pub version: String,
    pub candidate_id: Option<String>,
    pub build_id: String,
    pub source_fingerprint: Option<String>,
    pub channel: NativeChannel,
    pub app_name: String,
    pub identifier: String,
    pub executable_path: String,
    pub data_dir: String,
}

impl AppInfo {
    pub fn window_title(&self) -> String {
        format!(
            "{} · {}",
            self.channel.app_name(),
            self.channel.version_label(&self.version)
        )
    }
}

fn frozen_value(value: &str) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value.to_owned())
    }
}

pub(crate) fn app_info<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<AppInfo, String> {
    let identifier = app.config().identifier.clone();
    let channel = NativeChannel::from_identifier(&identifier)?;
    let executable_path =
        std::env::current_exe().map_err(|error| format!("无法读取正在运行的应用路径：{error}"))?;
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("无法读取应用数据目录：{error}"))?;
    let package = app.package_info();
    Ok(AppInfo {
        version: package.version.to_string(),
        candidate_id: frozen_value(env!("PIXEL_CANDIDATE_ID")),
        build_id: env!("PIXEL_COMPILED_BUILD_ID").into(),
        source_fingerprint: frozen_value(env!("PIXEL_SOURCE_FINGERPRINT")),
        channel,
        app_name: package.name.clone(),
        identifier,
        executable_path: executable_path.to_string_lossy().into_owned(),
        data_dir: data_dir.to_string_lossy().into_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channels_are_selected_only_by_the_exact_application_identifier() {
        assert_eq!(
            NativeChannel::from_identifier("dev.pixel.workspace.dev").unwrap(),
            NativeChannel::Dev
        );
        assert_eq!(
            NativeChannel::from_identifier("dev.pixel.workspace").unwrap(),
            NativeChannel::Beta
        );
        for identifier in [
            "",
            "dev.pixel.workspace.beta",
            "dev.pixel.workspace.dev.extra",
        ] {
            assert!(NativeChannel::from_identifier(identifier).is_err());
        }
    }

    #[test]
    fn native_version_labels_and_titles_include_the_channel() {
        for (channel, suffix, name) in [
            (NativeChannel::Dev, "dev", "Atrio WorkSpace Dev"),
            (NativeChannel::Beta, "beta", "Atrio WorkSpace Beta"),
        ] {
            assert_eq!(channel.version_label("0.1.2"), format!("0.1.2-{suffix}"));
            let info = AppInfo {
                version: "0.1.2".into(),
                candidate_id: Some("candidate-test".into()),
                build_id: "candidate-test-build".into(),
                source_fingerprint: Some(format!("sha256:{}", "a".repeat(64))),
                channel,
                app_name: name.into(),
                identifier: "test-only".into(),
                executable_path: "/test/app".into(),
                data_dir: "/test/data".into(),
            };
            assert_eq!(info.window_title(), format!("{name} · 0.1.2-{suffix}"));
            let json = serde_json::to_value(&info).unwrap();
            assert_eq!(json["channel"], suffix);
            assert_eq!(json["appName"], name);
            assert_eq!(json["executablePath"], "/test/app");
            assert_eq!(json["dataDir"], "/test/data");
            assert_eq!(json["candidateId"], "candidate-test");
            assert_eq!(json["buildId"], "candidate-test-build");
            assert_eq!(
                json["sourceFingerprint"],
                format!("sha256:{}", "a".repeat(64))
            );
        }
    }
}
