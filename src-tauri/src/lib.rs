mod agent;
mod identity;
mod kernel;
mod locks;
mod mcp_bridge;
mod platform;
mod policy;
mod workspace_tools;

pub fn run_workspace_mcp() -> Result<(), String> {
    mcp_bridge::run_stdio()
}

use identity::{app_info, AppInfo};
use kernel::{Kernel, RunStatus, WorkspaceAction, WorkspaceSnapshot};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
    time::Duration,
};
use tauri::{Emitter, Manager, State};

struct WorkspaceState {
    kernel: Mutex<Result<Kernel, String>>,
    selection_path: PathBuf,
    policy: Mutex<policy::PolicyStore>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceSelection {
    root: PathBuf,
}

struct ToolBridgePaths {
    socket: PathBuf,
    executable: PathBuf,
}

fn configure_tools(app: &tauri::AppHandle, kernel: &mut Kernel) {
    if let Some(paths) = app.try_state::<ToolBridgePaths>() {
        kernel.configure_tool_bridge(paths.socket.clone(), paths.executable.clone());
    }
}

fn load_workspace_root(config: &Path, default: PathBuf) -> Result<PathBuf, String> {
    if !config.exists() {
        return Ok(default);
    }
    let bytes = fs::read(config).map_err(|e| format!("无法读取工作区设置：{e}"))?;
    let selection: WorkspaceSelection =
        serde_json::from_slice(&bytes).map_err(|e| format!("工作区设置损坏：{e}"))?;
    Ok(selection.root)
}

fn save_workspace_root(config: &Path, root: &Path) -> Result<(), String> {
    let selection = WorkspaceSelection {
        root: root.to_owned(),
    };
    let bytes = serde_json::to_vec_pretty(&selection).map_err(|e| e.to_string())?;
    let temp = config.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|e| format!("无法保存工作区设置：{e}"))?;
        file.write_all(&bytes)
            .map_err(|e| format!("无法写入工作区设置：{e}"))?;
        file.sync_all()
            .map_err(|e| format!("无法同步工作区设置：{e}"))?;
        drop(file);
        #[cfg(windows)]
        let replacement = platform::commit_replace(&temp, config);
        #[cfg(not(windows))]
        let replacement = fs::rename(&temp, config);
        replacement.map_err(|e| format!("无法更新工作区设置：{e}"))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

#[tauri::command]
fn get_app_info(app: tauri::AppHandle) -> Result<AppInfo, String> {
    app_info(&app)
}

#[tauri::command]
fn get_workspace(state: State<'_, WorkspaceState>) -> Result<WorkspaceSnapshot, String> {
    let mut kernel = state
        .kernel
        .lock()
        .map_err(|_| "工作区状态锁异常".to_owned())?;
    let kernel = kernel.as_mut().map_err(|e| e.clone())?;
    let mut policy = state.policy.lock().map_err(|_| "权限设置锁异常")?;
    sync_policy(kernel, &mut policy)?;
    kernel.snapshot()
}

fn sync_policy(kernel: &mut Kernel, store: &mut policy::PolicyStore) -> Result<bool, String> {
    let provider = kernel.selected_provider_id().to_owned();
    let (root, id, restricted) = kernel.policy_identity();
    let next = store.workspace_for_provider(root, id, restricted, &provider)?;
    kernel.apply_policy(next)
}

#[tauri::command]
fn open_workspace(
    app: tauri::AppHandle,
    state: State<'_, WorkspaceState>,
    path: String,
) -> Result<WorkspaceSnapshot, String> {
    app.state::<locks::ChannelInstanceLock>().ensure_valid()?;
    let mut active = state
        .kernel
        .lock()
        .map_err(|_| "工作区状态锁异常".to_owned())?;
    switch_workspace(&mut active, &state.selection_path, &path)?;
    let mut store = state.policy.lock().map_err(|_| "权限设置锁异常")?;
    let kernel = active.as_mut().map_err(|e| e.clone())?;
    configure_tools(&app, kernel);
    sync_policy(kernel, &mut store)?;
    let snapshot = kernel.snapshot()?;
    // The command response is authoritative even if event delivery fails.
    // Only announce the newly active workspace, never the previous cancellation.
    if let Err(error) = app.emit("workspace://changed", &snapshot) {
        eprintln!("workspace switch event delivery failed: {error}");
    }
    Ok(snapshot)
}

fn switch_workspace(
    active: &mut Result<Kernel, String>,
    selection_path: &Path,
    path: &str,
) -> Result<WorkspaceSnapshot, String> {
    if path.trim().is_empty() {
        return Err("请选择工作区文件夹".to_owned());
    }
    let requested_root = PathBuf::from(path);
    if !requested_root.is_absolute() {
        return Err("工作区必须使用绝对路径".to_owned());
    }
    if let (Ok(root), Ok(previous)) = (requested_root.canonicalize(), active.as_ref()) {
        let snapshot = previous.snapshot()?;
        if root == Path::new(&snapshot.root_path) {
            return Ok(snapshot);
        }
    }
    let kernel = Kernel::open(&requested_root)?;
    let root = requested_root
        .canonicalize()
        .map_err(|e| format!("无法解析工作区路径：{e}"))?;
    let snapshot = kernel.snapshot()?;
    if let Ok(previous) = active.as_mut() {
        if matches!(previous.snapshot()?.run_status, RunStatus::Running) {
            previous.dispatch(WorkspaceAction::Cancel)?;
        }
        previous.shutdown()?;
    }
    save_workspace_root(selection_path, &root)?;
    *active = Ok(kernel);
    Ok(snapshot)
}

fn dispatch_in_workspace(
    kernel: &mut Kernel,
    expected_root: &str,
    expected_generation: &str,
    action: WorkspaceAction,
) -> Result<WorkspaceSnapshot, String> {
    let current = kernel.snapshot()?;
    if current.root_path != expected_root
        || expected_generation.trim().is_empty()
        || current.workspace_generation != expected_generation
    {
        return Err("工作区已切换，此操作已取消；请在当前工作区重新操作".into());
    }
    kernel.dispatch(action)
}

#[tauri::command]
fn dispatch(
    app: tauri::AppHandle,
    state: State<'_, WorkspaceState>,
    expected_root: String,
    expected_generation: String,
    action: WorkspaceAction,
) -> Result<WorkspaceSnapshot, String> {
    app.state::<locks::ChannelInstanceLock>().ensure_valid()?;
    let mut active = state
        .kernel
        .lock()
        .map_err(|_| "工作区状态锁异常".to_owned())?;
    let kernel = active.as_mut().map_err(|e| e.clone())?;
    let current = kernel.snapshot()?;
    if current.root_path != expected_root || current.workspace_generation != expected_generation {
        return Err("工作区已切换，此操作已取消".into());
    }
    let mut store = state.policy.lock().map_err(|_| "权限设置锁异常")?;
    sync_policy(kernel, &mut store)?;
    let root = PathBuf::from(&current.root_path);
    let workspace_id = current.workspace_id.clone();
    let result = match action {
        WorkspaceAction::SetWorkspacePolicy { mode } => store
            .set_workspace(&root, &workspace_id, mode)
            .and_then(|_| sync_policy(kernel, &mut store))
            .and_then(|_| kernel.snapshot()),
        WorkspaceAction::SetSystemPolicy { mode } => store
            .set_system(mode)
            .and_then(|_| sync_policy(kernel, &mut store))
            .and_then(|_| kernel.snapshot()),
        WorkspaceAction::ConfirmHermesScope { accepted }
        | WorkspaceAction::ConfirmAgentScope { accepted } => {
            store
                .accept_agent_scope(
                    &root,
                    &workspace_id,
                    current.agent.provider.as_deref().unwrap_or(
                        if current.agent.transport == "mock" {
                            "mock"
                        } else {
                            "hermes"
                        },
                    ),
                    accepted,
                )
                .and_then(|p| kernel.apply_policy(p))
                .and_then(|_| kernel.snapshot())
        }
        WorkspaceAction::SetPermissionMode { mode } => store
            .set_workspace(
                &root,
                &workspace_id,
                if mode == kernel::PermissionMode::Restricted {
                    policy::WorkspacePolicy::Restricted
                } else {
                    policy::WorkspacePolicy::Ask
                },
            )
            .and_then(|_| sync_policy(kernel, &mut store))
            .and_then(|_| kernel.snapshot()),
        action => {
            let agent_changed = matches!(&action, WorkspaceAction::SaveAgent{agent} if agent.command!=current.agent.command||agent.args!=current.agent.args||agent.transport!=current.agent.transport||agent.provider!=current.agent.provider||agent.env!=current.agent.env||agent.cwd!=current.agent.cwd);
            let result =
                dispatch_in_workspace(kernel, &expected_root, &expected_generation, action);
            if result.is_ok() && agent_changed {
                store.accept_agent_scope(&root, &workspace_id, "hermes", false)?;
                sync_policy(kernel, &mut store)?;
                kernel.snapshot()
            } else {
                result
            }
        }
    };
    if result.is_err() {
        let _ = sync_policy(kernel, &mut store);
    }
    let snapshot = match &result {
        Ok(snapshot) => Some(snapshot.clone()),
        Err(error) => Some(
            kernel
                .snapshot()
                .map_err(|snapshot_error| format!("{error}；无法刷新状态：{snapshot_error}"))?,
        ),
    };
    if let Some(snapshot) = snapshot {
        if let Err(error) = app.emit("workspace://changed", &snapshot) {
            return Err(format!("操作状态已保存，但界面通知失败：{error}"));
        }
    }
    result
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            get_app_info,
            get_workspace,
            open_workspace,
            dispatch
        ])
        .setup(|app| {
            let info = app_info(app.handle()).map_err(std::io::Error::other)?;
            for window in app.webview_windows().values() {
                window.set_title(&info.window_title())?;
            }
            let data = app.path().app_data_dir()?;
            fs::create_dir_all(&data)?;
            // A channel lock is separate from the physical workspace lock.
            // Keep it managed for the entire application lifetime.
            match locks::ChannelInstanceLock::acquire(&data) {
                Ok(guard) => {
                    app.manage(guard);
                }
                Err(error) => {
                    eprintln!("{error}");
                    app.handle().exit(0);
                    return Ok(());
                }
            }
            let selection_path = data.join("workspace-selection.json");
            let mut policy = policy::PolicyStore::open(&data.join("agent-policy.json"))
                .map_err(std::io::Error::other)?;
            let mut initial = load_workspace_root(&selection_path, data.join("Workspace"))
                .and_then(|root| Kernel::open(&root));
            if let Ok(kernel) = initial.as_mut() {
                if let Err(error) = sync_policy(kernel, &mut policy) {
                    initial = Err(error);
                }
            }
            app.manage(WorkspaceState {
                kernel: Mutex::new(initial),
                selection_path,
                policy: Mutex::new(policy),
            });

            let tool_handle = app.handle().clone();
            let tool_server = mcp_bridge::ToolServer::start(std::sync::Arc::new(move |request| {
                tool_handle
                    .state::<locks::ChannelInstanceLock>()
                    .ensure_valid()?;
                let state = tool_handle.state::<WorkspaceState>();
                let mut active = state.kernel.lock().map_err(|_| "工作区状态锁异常")?;
                let kernel = active.as_mut().map_err(|e| e.clone())?;
                let mut store = state.policy.lock().map_err(|_| "权限设置锁异常")?;
                sync_policy(kernel, &mut store)?;
                let result = kernel.handle_module_tool_request(
                    &request.session_token,
                    &request.run_scope,
                    &request.tool,
                    request.args,
                    &request.request_id,
                );
                if let Ok(snapshot) = kernel.snapshot() {
                    let _ = tool_handle.emit("workspace://changed", snapshot);
                }
                result
            }))
            .map_err(std::io::Error::other)?;
            app.manage(ToolBridgePaths {
                socket: tool_server.path().to_owned(),
                executable: std::env::current_exe()?,
            });
            app.manage(tool_server);
            {
                let state = app.state::<WorkspaceState>();
                let mut active = state
                    .kernel
                    .lock()
                    .map_err(|_| std::io::Error::other("工作区状态锁异常"))?;
                if let Ok(kernel) = active.as_mut() {
                    configure_tools(app.handle(), kernel);
                }
            }

            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let mut last_error = None;
                loop {
                    std::thread::sleep(Duration::from_millis(150));
                    let state = handle.state::<WorkspaceState>();
                    let mut active = match state.kernel.lock() {
                        Ok(active) => active,
                        Err(_) => {
                            let error = "工作区状态锁异常";
                            eprintln!("{error}");
                            let _ = handle.emit("workspace://error", error);
                            break;
                        }
                    };
                    let update = match active.as_mut() {
                        Ok(kernel) => match state.policy.lock() {
                            Ok(mut store) => sync_policy(kernel, &mut store).and_then(|changed| {
                                kernel.tick().and_then(|update| {
                                    if changed && update.is_none() {
                                        kernel.snapshot().map(Some)
                                    } else {
                                        Ok(update)
                                    }
                                })
                            }),
                            Err(_) => Err("权限设置锁异常".into()),
                        },
                        Err(_) => continue, // The initial load error is returned by get_workspace.
                    };
                    match update {
                        Ok(Some(snapshot)) => {
                            last_error = None;
                            if let Err(error) = handle.emit("workspace://changed", snapshot) {
                                eprintln!("workspace event delivery failed: {error}");
                            }
                        }
                        Ok(None) => {
                            last_error = None;
                        }
                        Err(error) => {
                            if last_error.as_ref() != Some(&error) {
                                eprintln!("workspace tick failed: {error}");
                                if let Err(delivery) = handle.emit("workspace://error", &error) {
                                    eprintln!("workspace error delivery failed: {delivery}");
                                }
                                last_error = Some(error);
                            }
                        }
                    }
                }
            });
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("Atrio WorkSpace could not start")
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { api, .. } = &event {
                if let Some(state) = app.try_state::<WorkspaceState>() {
                    if let Ok(mut active) = state.kernel.lock() {
                        if let Ok(kernel) = active.as_mut() {
                            if let Err(error) = kernel.shutdown() {
                                eprintln!("停止失败，应用保持打开：{error}");
                                let _ = app.emit("workspace://error", &error);
                                api.prevent_exit();
                            }
                        }
                    }
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernel::ModuleType;

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir()
                .join(format!("pixel-workspace-switch-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn selection(&self) -> PathBuf {
            self.0.join("selection.json")
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn switching_cancels_previous_run_and_restores_selected_workspace() {
        let fixture = Fixture::new();
        let old_path = fixture.0.join("old");
        let new_path = fixture.0.join("new");
        let mut old = Kernel::open(&old_path).unwrap();
        old.dispatch(WorkspaceAction::Prompt {
            text: "长任务".into(),
            module_id: None,
        })
        .unwrap();
        old.tick().unwrap();
        let old_root = old.snapshot().unwrap().root_path;
        let mut active = Ok(old);
        let opened = switch_workspace(
            &mut active,
            &fixture.selection(),
            new_path.to_str().unwrap(),
        )
        .unwrap();
        assert_ne!(opened.root_path, old_root);
        assert_eq!(
            active.as_ref().unwrap().snapshot().unwrap().root_path,
            opened.root_path
        );
        assert!(active.as_mut().unwrap().tick().unwrap().is_none());

        let old = Kernel::open(&old_path).unwrap().snapshot().unwrap();
        assert_eq!(old.run_status, RunStatus::Cancelled);
        assert!(old
            .events
            .iter()
            .any(|event| event.kind == "session/cancel"));
        let selected = load_workspace_root(&fixture.selection(), fixture.0.join("unused")).unwrap();
        assert_eq!(selected, PathBuf::from(&opened.root_path));
        drop(active);
        let restored = Kernel::open(&selected).unwrap().snapshot().unwrap();
        assert_eq!(restored.modules[0].id, opened.modules[0].id);
    }

    #[test]
    fn old_workspace_actions_cannot_mutate_the_new_kernel() {
        let fixture = Fixture::new();
        let mut active = Ok(Kernel::open(&fixture.0.join("old")).unwrap());
        let old = active.as_ref().unwrap().snapshot().unwrap();
        let new_path = fixture.0.join("new");
        let opened = switch_workspace(
            &mut active,
            &fixture.selection(),
            new_path.to_str().unwrap(),
        )
        .unwrap();
        let kernel = active.as_mut().unwrap();
        // An action without a module ID would otherwise succeed in the wrong workspace.
        assert!(dispatch_in_workspace(
            kernel,
            &old.root_path,
            &old.workspace_generation,
            WorkspaceAction::CreateModule {
                module_type: ModuleType::Document,
                title: "stale action".into(),
            }
        )
        .unwrap_err()
        .contains("工作区已切换"));
        assert_eq!(
            kernel.snapshot().unwrap().modules.len(),
            opened.modules.len()
        );
        assert_eq!(fs::read_dir(new_path.join("notes")).unwrap().count(), 1);
        dispatch_in_workspace(
            kernel,
            &opened.root_path,
            &opened.workspace_generation,
            WorkspaceAction::CreateModule {
                module_type: ModuleType::Planner,
                title: "current action".into(),
            },
        )
        .unwrap();
        assert_eq!(
            kernel.snapshot().unwrap().modules.len(),
            opened.modules.len() + 1
        );
    }

    #[test]
    fn aba_switch_rejects_old_generation_even_with_the_same_root_and_module_ids() {
        let fixture = Fixture::new();
        let first_path = fixture.0.join("a");
        let second_path = fixture.0.join("b");
        let mut active = Ok(Kernel::open(&first_path).unwrap());
        let first = active.as_ref().unwrap().snapshot().unwrap();
        switch_workspace(
            &mut active,
            &fixture.selection(),
            second_path.to_str().unwrap(),
        )
        .unwrap();
        let reopened = switch_workspace(
            &mut active,
            &fixture.selection(),
            first_path.to_str().unwrap(),
        )
        .unwrap();
        assert_eq!(first.root_path, reopened.root_path);
        assert_eq!(first.modules[0].id, reopened.modules[0].id);
        assert_ne!(first.workspace_generation, reopened.workspace_generation);
        let kernel = active.as_mut().unwrap();
        for generation in [first.workspace_generation.as_str(), ""] {
            assert!(dispatch_in_workspace(
                kernel,
                &first.root_path,
                generation,
                WorkspaceAction::RenameModule {
                    module_id: first.modules[0].id.clone(),
                    title: "stale rename".into(),
                },
            )
            .unwrap_err()
            .contains("工作区已切换"));
        }
        assert_eq!(
            kernel.snapshot().unwrap().modules[0].title,
            reopened.modules[0].title
        );
        dispatch_in_workspace(
            kernel,
            &reopened.root_path,
            &reopened.workspace_generation,
            WorkspaceAction::RenameModule {
                module_id: reopened.modules[0].id.clone(),
                title: "current rename".into(),
            },
        )
        .unwrap();
        assert_eq!(
            kernel.snapshot().unwrap().modules[0].title,
            "current rename"
        );
    }

    #[test]
    fn failed_or_same_directory_switch_keeps_the_existing_kernel() {
        let fixture = Fixture::new();
        let old_path = fixture.0.join("old");
        let mut active = Ok(Kernel::open(&old_path).unwrap());
        let old = active.as_ref().unwrap().snapshot().unwrap();
        active
            .as_mut()
            .unwrap()
            .dispatch(WorkspaceAction::Prompt {
                text: "长任务".into(),
                module_id: None,
            })
            .unwrap();
        let same = switch_workspace(
            &mut active,
            &fixture.selection(),
            old_path.to_str().unwrap(),
        )
        .unwrap();
        assert_eq!(same.run_status, RunStatus::Running);

        let not_directory = fixture.0.join("file");
        fs::write(&not_directory, "not a workspace").unwrap();
        assert!(switch_workspace(
            &mut active,
            &fixture.selection(),
            not_directory.to_str().unwrap(),
        )
        .is_err());
        assert_eq!(
            active.as_ref().unwrap().snapshot().unwrap().run_status,
            RunStatus::Running
        );
        assert_eq!(
            active.as_ref().unwrap().snapshot().unwrap().root_path,
            old.root_path
        );

        let bad_selection = fixture.0.join("missing-parent/selection.json");
        assert!(switch_workspace(
            &mut active,
            &bad_selection,
            fixture.0.join("new").to_str().unwrap(),
        )
        .is_err());
        assert_eq!(
            active.as_ref().unwrap().snapshot().unwrap().root_path,
            old.root_path
        );
    }
}
