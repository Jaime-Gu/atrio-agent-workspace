//! Channel-local authorization. Workspace files can never grant themselves trust.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspacePolicy {
    Disabled,
    Restricted,
    Ask,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemPolicy {
    Workspace,
    AllowAll,
    DenyAll,
}

pub fn effective(system: SystemPolicy, local: WorkspacePolicy) -> WorkspacePolicy {
    match system {
        SystemPolicy::AllowAll => WorkspacePolicy::Full,
        SystemPolicy::DenyAll => WorkspacePolicy::Disabled,
        SystemPolicy::Workspace => local,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicySnapshot {
    pub system: SystemPolicy,
    pub local: WorkspacePolicy,
    pub effective: WorkspacePolicy,
    pub source: String,
    pub epoch: u64,
    pub workspace_trusted: bool,
    pub hermes_scope_accepted: bool,
    #[serde(default)]
    pub agent_scope_accepted: bool,
    #[serde(default)]
    pub scope_provider: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LocalPolicy {
    mode: WorkspacePolicy,
    epoch: u64,
    trusted: bool,
    hermes_scope_accepted: bool,
    #[serde(default)]
    accepted_provider: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PolicyData {
    schema_version: u32,
    system: SystemPolicy,
    workspaces: BTreeMap<String, LocalPolicy>,
}

pub struct PolicyStore {
    path: PathBuf,
    data: PolicyData,
}

impl PolicyStore {
    pub fn open(path: &Path) -> Result<Self, String> {
        let data = if path.exists() {
            let bytes = fs::read(path).map_err(|e| format!("无法读取权限设置：{e}"))?;
            if bytes.len() > 2_000_000 {
                return Err("权限设置文件过大；已阻止 Agent 运行".into());
            }
            let data: PolicyData = serde_json::from_slice(&bytes)
                .map_err(|_| "权限设置损坏；已阻止 Agent 运行，请恢复设置备份".to_string())?;
            if data.schema_version != 1 {
                return Err("权限设置版本不受支持；已阻止 Agent 运行".into());
            }
            data
        } else {
            PolicyData {
                schema_version: 1,
                system: SystemPolicy::Workspace,
                workspaces: BTreeMap::new(),
            }
        };
        Ok(Self {
            path: path.to_owned(),
            data,
        })
    }

    pub fn workspace(
        &mut self,
        root: &Path,
        workspace_id: &str,
        legacy_restricted: bool,
    ) -> Result<PolicySnapshot, String> {
        let key = workspace_key(root, workspace_id)?;
        if !self.data.workspaces.contains_key(&key) {
            let mut data = self.data.clone();
            data.workspaces.insert(
                key.clone(),
                LocalPolicy {
                    mode: if legacy_restricted {
                        WorkspacePolicy::Restricted
                    } else {
                        WorkspacePolicy::Ask
                    },
                    epoch: 1,
                    trusted: false,
                    hermes_scope_accepted: false,
                    accepted_provider: None,
                },
            );
            self.commit(data)?;
        }
        Ok(self.snapshot(&key))
    }

    pub fn set_workspace(
        &mut self,
        root: &Path,
        id: &str,
        mode: WorkspacePolicy,
    ) -> Result<PolicySnapshot, String> {
        let key = workspace_key(root, id)?;
        let mut data = self.data.clone();
        let local = data
            .workspaces
            .get_mut(&key)
            .ok_or("工作区权限尚未初始化")?;
        if effective(data.system, local.mode) != effective(data.system, mode) {
            local.epoch += 1;
        }
        local.mode = mode;
        local.trusted = true;
        self.commit(data)?;
        Ok(self.snapshot(&key))
    }

    pub fn set_system(&mut self, mode: SystemPolicy) -> Result<(), String> {
        let mut data = self.data.clone();
        for local in data.workspaces.values_mut() {
            if effective(data.system, local.mode) != effective(mode, local.mode) {
                local.epoch += 1;
            }
        }
        data.system = mode;
        self.commit(data)
    }

    pub fn accept_hermes_scope(
        &mut self,
        root: &Path,
        id: &str,
        accepted: bool,
    ) -> Result<PolicySnapshot, String> {
        self.accept_agent_scope(root, id, "hermes", accepted)
    }

    pub fn workspace_for_provider(
        &mut self,
        root: &Path,
        id: &str,
        legacy_restricted: bool,
        provider: &str,
    ) -> Result<PolicySnapshot, String> {
        let mut snapshot = self.workspace(root, id, legacy_restricted)?;
        snapshot.agent_scope_accepted = snapshot.scope_provider.as_deref() == Some(provider);
        Ok(snapshot)
    }

    pub fn accept_agent_scope(
        &mut self,
        root: &Path,
        id: &str,
        provider: &str,
        accepted: bool,
    ) -> Result<PolicySnapshot, String> {
        if !["mock", "hermes", "claude_code", "codex"].contains(&provider) {
            return Err("Agent Provider身份无效，不能授权".into());
        }
        let key = workspace_key(root, id)?;
        let mut data = self.data.clone();
        let local = data
            .workspaces
            .get_mut(&key)
            .ok_or("工作区权限尚未初始化")?;
        let previous = local
            .accepted_provider
            .clone()
            .or_else(|| local.hermes_scope_accepted.then(|| "hermes".into()));
        let next = accepted.then(|| provider.to_owned());
        if previous != next && effective(data.system, local.mode) == WorkspacePolicy::Ask {
            local.epoch += 1;
        }
        local.hermes_scope_accepted = accepted && provider == "hermes";
        local.accepted_provider = next;
        local.trusted = true;
        self.commit(data)?;
        let mut snapshot = self.snapshot(&key);
        snapshot.agent_scope_accepted = snapshot.scope_provider.as_deref() == Some(provider);
        Ok(snapshot)
    }

    fn snapshot(&self, key: &str) -> PolicySnapshot {
        let local = &self.data.workspaces[key];
        PolicySnapshot {
            system: self.data.system,
            local: local.mode,
            effective: effective(self.data.system, local.mode),
            source: if self.data.system == SystemPolicy::Workspace {
                "workspace"
            } else {
                "system"
            }
            .into(),
            epoch: local.epoch,
            workspace_trusted: local.trusted,
            hermes_scope_accepted: local.hermes_scope_accepted,
            scope_provider: local
                .accepted_provider
                .clone()
                .or_else(|| local.hermes_scope_accepted.then(|| "hermes".into())),
            agent_scope_accepted: local.accepted_provider.is_some() || local.hermes_scope_accepted,
        }
    }

    fn commit(&mut self, data: PolicyData) -> Result<(), String> {
        let parent = self.path.parent().ok_or("权限设置缺少父目录")?;
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let temp = parent.join(format!(".policy-{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)
                .map_err(|e| e.to_string())?;
            file.write_all(&serde_json::to_vec_pretty(&data).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            file.sync_all().map_err(|e| e.to_string())?;
            fs::rename(&temp, &self.path).map_err(|e| e.to_string())?;
            fs::File::open(parent)
                .and_then(|f| f.sync_all())
                .map_err(|e| e.to_string())?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(temp);
            // A failed persistence attempt cannot leave a live Agent relying
            // on possibly stale authorization. The Host resynchronizes this
            // in-memory denial and reports the original write error.
            for local in self.data.workspaces.values_mut() {
                if effective(self.data.system, local.mode) != WorkspacePolicy::Disabled {
                    local.epoch += 1;
                }
            }
            self.data.system = SystemPolicy::DenyAll;
        } else {
            self.data = data;
        }
        result
    }
}

fn workspace_key(root: &Path, id: &str) -> Result<String, String> {
    if uuid::Uuid::parse_str(id).is_err() {
        return Err("工作区身份未核实，禁止 Agent 运行".into());
    }
    let root = fs::canonicalize(root).map_err(|e| e.to_string())?;
    let metadata = fs::metadata(&root).map_err(|e| e.to_string())?;
    if !metadata.is_dir() {
        return Err("工作区身份无效".into());
    }
    #[cfg(unix)]
    let identity = {
        use std::os::unix::fs::MetadataExt;
        format!(
            "{}\0{}\0{}\0{}",
            root.display(),
            metadata.dev(),
            metadata.ino(),
            id
        )
    };
    #[cfg(not(unix))]
    let identity = format!("{}\0{}", root.display(), id);
    Ok(format!("sha256:{:x}", Sha256::digest(identity.as_bytes())))
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("pixel-policy-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn scope_acceptance_is_bound_to_provider_and_legacy_hermes_does_not_authorize_other_adapters() {
        let f = Fixture::new();
        let id = uuid::Uuid::new_v4().to_string();
        let mut store = PolicyStore::open(&f.0.join("policy.json")).unwrap();
        store.workspace(&f.0, &id, false).unwrap();
        let hermes = store.accept_hermes_scope(&f.0, &id, true).unwrap();
        assert!(hermes.agent_scope_accepted);
        let claude = store
            .workspace_for_provider(&f.0, &id, false, "claude_code")
            .unwrap();
        assert!(!claude.agent_scope_accepted);
        assert_eq!(claude.scope_provider.as_deref(), Some("hermes"));
        let claude = store
            .accept_agent_scope(&f.0, &id, "claude_code", true)
            .unwrap();
        assert!(claude.agent_scope_accepted);
        assert!(!claude.hermes_scope_accepted);
        assert!(claude.epoch > hermes.epoch);
        assert!(
            !store
                .workspace_for_provider(&f.0, &id, false, "codex")
                .unwrap()
                .agent_scope_accepted
        );
        assert!(
            !store
                .workspace_for_provider(&f.0, &id, false, "hermes")
                .unwrap()
                .agent_scope_accepted
        );
    }

    #[test]
    fn entire_effective_matrix() {
        for local in [
            WorkspacePolicy::Disabled,
            WorkspacePolicy::Restricted,
            WorkspacePolicy::Ask,
            WorkspacePolicy::Full,
        ] {
            assert_eq!(effective(SystemPolicy::Workspace, local), local);
            assert_eq!(
                effective(SystemPolicy::AllowAll, local),
                WorkspacePolicy::Full
            );
            assert_eq!(
                effective(SystemPolicy::DenyAll, local),
                WorkspacePolicy::Disabled
            );
        }
    }
    #[test]
    fn covered_local_changes_preserve_epoch_and_restore_preference() {
        let f = Fixture::new();
        let id = uuid::Uuid::new_v4().to_string();
        let mut s = PolicyStore::open(&f.0.join("policy.json")).unwrap();
        let a = s.workspace(&f.0, &id, false).unwrap();
        s.set_system(SystemPolicy::AllowAll).unwrap();
        let b = s.workspace(&f.0, &id, false).unwrap();
        assert!(b.epoch > a.epoch);
        let c = s
            .set_workspace(&f.0, &id, WorkspacePolicy::Restricted)
            .unwrap();
        assert_eq!(c.epoch, b.epoch);
        assert_eq!(c.effective, WorkspacePolicy::Full);
        s.set_system(SystemPolicy::Workspace).unwrap();
        let d = s.workspace(&f.0, &id, false).unwrap();
        assert_eq!(d.effective, WorkspacePolicy::Restricted);
        assert!(d.epoch > c.epoch);
    }
    #[test]
    fn copies_and_moved_paths_do_not_inherit_local_trust() {
        let f = Fixture::new();
        let a = f.0.join("a");
        let b = f.0.join("b");
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let mut s = PolicyStore::open(&f.0.join("policy.json")).unwrap();
        s.workspace(&a, &id, false).unwrap();
        s.set_workspace(&a, &id, WorkspacePolicy::Full).unwrap();
        let copy = s.workspace(&b, &id, false).unwrap();
        assert_eq!(copy.effective, WorkspacePolicy::Ask);
        assert!(!copy.workspace_trusted);
        s.set_system(SystemPolicy::AllowAll).unwrap();
        let copy = s.workspace(&b, &id, false).unwrap();
        assert_eq!(copy.effective, WorkspacePolicy::Full);
        assert_eq!(copy.local, WorkspacePolicy::Ask);
        assert!(!copy.workspace_trusted);
    }
    #[test]
    fn restrictive_legacy_and_channel_settings_are_preserved() {
        let f = Fixture::new();
        let id = uuid::Uuid::new_v4().to_string();
        let mut dev = PolicyStore::open(&f.0.join("dev.json")).unwrap();
        let mut beta = PolicyStore::open(&f.0.join("beta.json")).unwrap();
        assert_eq!(
            dev.workspace(&f.0, &id, true).unwrap().local,
            WorkspacePolicy::Restricted
        );
        beta.workspace(&f.0, &id, false).unwrap();
        dev.set_system(SystemPolicy::DenyAll).unwrap();
        assert_eq!(
            beta.workspace(&f.0, &id, false).unwrap().effective,
            WorkspacePolicy::Ask
        );
        drop(dev);
        let mut dev = PolicyStore::open(&f.0.join("dev.json")).unwrap();
        assert_eq!(
            dev.workspace(&f.0, &id, false).unwrap().effective,
            WorkspacePolicy::Disabled
        );
    }
    #[test]
    fn privilege_escalation_and_scope_revocation_advance_epoch() {
        let f = Fixture::new();
        let id = uuid::Uuid::new_v4().to_string();
        let mut s = PolicyStore::open(&f.0.join("policy.json")).unwrap();
        let a = s.workspace(&f.0, &id, false).unwrap();
        let b = s.accept_hermes_scope(&f.0, &id, true).unwrap();
        assert!(b.epoch > a.epoch);
        let c = s.accept_hermes_scope(&f.0, &id, false).unwrap();
        assert!(c.epoch > b.epoch);
        let d = s.set_workspace(&f.0, &id, WorkspacePolicy::Full).unwrap();
        assert!(d.epoch > c.epoch);
        let e = s.set_workspace(&f.0, &id, WorkspacePolicy::Full).unwrap();
        assert_eq!(e.epoch, d.epoch);
    }
    #[test]
    fn corrupt_or_unknown_identity_is_never_allowed() {
        let f = Fixture::new();
        let p = f.0.join("policy.json");
        fs::write(&p, "{bad}").unwrap();
        assert!(PolicyStore::open(&p).is_err());
        let mut s = PolicyStore::open(&f.0.join("fresh.json")).unwrap();
        assert!(s.workspace(&f.0, "unverified", false).is_err());
    }
}
