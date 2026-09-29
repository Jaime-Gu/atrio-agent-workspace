//! Deterministic diagnostic agent. It only emits proposals; it owns no files,
//! database, policy decisions, or approval state.
use super::*;
use chrono::Utc;
use std::collections::VecDeque;

enum Intent {
    Chat,
    Create(ModuleType),
    Write(ModuleContext),
    Fail,
}

struct MockRun {
    context: ConnectorContext,
    chunks: VecDeque<String>,
    intent: Intent,
}

#[derive(Default)]
pub struct MockAgentConnector {
    initialized: bool,
    session: Option<(String, String)>,
    session_persisted: bool,
    events: VecDeque<ConnectorEvent>,
    run: Option<MockRun>,
    seq: u64,
}

impl MockAgentConnector {
    fn event(&mut self, context: &ConnectorContext, kind: ConnectorEventKind) -> ConnectorEvent {
        self.seq += 1;
        ConnectorEvent {
            context: context.clone(),
            seq: self.seq,
            kind,
        }
    }
}

impl AgentConnector for MockAgentConnector {
    fn initialize(&mut self, context: &ConnectorContext) -> Result<(), String> {
        self.initialized = true;
        let event = self.event(context, ConnectorEventKind::Initialized {
            protocol_version: 1,
            capabilities: serde_json::json!({"mock": true, "hostProposals": true, "terminal": false, "network": false}),
        });
        self.events.push_back(event);
        Ok(())
    }

    fn new_session(&mut self, context: &ConnectorContext) -> Result<(), String> {
        if !self.initialized {
            return Err("Mock Connector 尚未初始化".into());
        }
        let provider_session_id = format!("mock-{}", context.runtime_session_id);
        self.session = Some((
            context.runtime_session_id.clone(),
            provider_session_id.clone(),
        ));
        self.session_persisted = false;
        let event = self.event(
            context,
            ConnectorEventKind::SessionCreated {
                provider_session_id,
            },
        );
        self.events.push_back(event);
        Ok(())
    }

    fn confirm_session_persisted(
        &mut self,
        context: &ConnectorContext,
        provider_session_id: &str,
    ) -> Result<(), String> {
        if self.session.as_ref().is_none_or(|(runtime, provider)| {
            runtime != &context.runtime_session_id || provider != provider_session_id
        }) {
            return Err("Mock 会话持久化确认不匹配".into());
        }
        self.session_persisted = true;
        Ok(())
    }

    fn prompt(&mut self, prompt: ConnectorPrompt) -> Result<(), String> {
        if self.run.is_some() {
            return Err("Mock Connector 已有运行中的任务".into());
        }
        if self
            .session
            .as_ref()
            .is_none_or(|(runtime, _)| runtime != &prompt.context.runtime_session_id)
        {
            return Err("Mock 会话不存在".into());
        }
        let (intent, answer, long) = classify_prompt(&prompt)?;
        let answer = if long {
            format!(
                "{}\n{}",
                answer,
                "正在逐步整理任务，期间可以点击取消。".repeat(28)
            )
        } else {
            answer
        };
        let characters: Vec<char> = answer.chars().collect();
        let chunks = characters
            .chunks(if long { 6 } else { 8 })
            .map(|characters| characters.iter().collect())
            .collect();
        self.run = Some(MockRun {
            context: prompt.context,
            chunks,
            intent,
        });
        Ok(())
    }

    fn cancel(&mut self, context: &ConnectorContext) -> Result<(), String> {
        if self.run.as_ref().is_some_and(|run| run.context == *context) {
            self.run = None;
            self.events.retain(|event| event.context != *context);
            let event = self.event(context, ConnectorEventKind::Cancelled);
            self.events.push_back(event);
        }
        Ok(())
    }

    fn dispose(&mut self) -> Result<(), String> {
        self.run = None;
        self.events.clear();
        self.session = None;
        self.session_persisted = false;
        self.initialized = false;
        Ok(())
    }

    fn poll_events(&mut self) -> Result<Vec<ConnectorEvent>, String> {
        if !self.events.is_empty() {
            return Ok(self.events.drain(..).collect());
        }
        if !self.session_persisted {
            return Ok(Vec::new());
        }
        let Some(mut run) = self.run.take() else {
            return Ok(Vec::new());
        };
        if let Some(text) = run.chunks.pop_front() {
            let event = self.event(&run.context, ConnectorEventKind::TextDelta { text });
            self.run = Some(run);
            return Ok(vec![event]);
        }
        let mut events = Vec::new();
        let kind = match run.intent {
            Intent::Fail => ConnectorEventKind::Failed {
                code: "MOCK_FIXTURE_ERROR".into(),
                message: "模拟 Agent 失败。可以继续发送任务，已有文件未改变".into(),
            },
            Intent::Chat => ConnectorEventKind::Completed {
                stop_reason: "end_turn".into(),
            },
            Intent::Create(module_type) => {
                let title = match module_type {
                    ModuleType::Planner => "本周行动计划",
                    ModuleType::Document => "Agent 草稿",
                    ModuleType::Dashboard => "任务数据看板",
                    ModuleType::Conversation => "新对话",
                }
                .to_string();
                let content = (module_type == ModuleType::Document).then(|| "# Agent 草稿\n\n这是一份本地 Mock 生成的 Markdown 草稿。\n\n## 下一步\n\n- 明确目标\n- 记录关键资料\n- 完成初稿并审阅\n".into());
                events.push(self.event(
                    &run.context,
                    ConnectorEventKind::ToolProposal {
                        proposal: WorkspaceToolIntent::ModuleProposal {
                            module_type,
                            title,
                            content,
                        },
                    },
                ));
                ConnectorEventKind::Completed {
                    stop_reason: "end_turn".into(),
                }
            }
            Intent::Write(document) => {
                let content = format!("{}\n\n## Agent 建议 · {}\n\n- 将目标拆分为三个可交付步骤。\n- 为关键任务安排专注时段。\n- 每周复盘并更新进度。\n\n> 本段由本地 Mock 生成，已通过文件修改审批后才会保存。\n", document.content.trim_end(), Utc::now().format("%Y-%m-%d %H:%M"));
                events.push(self.event(
                    &run.context,
                    ConnectorEventKind::ToolProposal {
                        proposal: WorkspaceToolIntent::WriteProposal {
                            module_id: document.id,
                            title: "Agent 提议修改文档".into(),
                            before_revision: document.revision.ok_or("文档尚无有效 revision")?,
                            content,
                        },
                    },
                ));
                ConnectorEventKind::Completed {
                    stop_reason: "end_turn".into(),
                }
            }
        };
        events.push(self.event(&run.context, kind));
        Ok(events)
    }

    fn permission_reply(
        &mut self,
        _context: &ConnectorContext,
        _request_id: &Value,
        _option_id: Option<&str>,
    ) -> Result<(), String> {
        Err("Mock 的工作区提案由 Host 审批，不使用 ACP permission request".into())
    }
}

fn classify_prompt(prompt: &ConnectorPrompt) -> Result<(Intent, String, bool), String> {
    let lower = prompt.text.to_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|needle| lower.contains(needle));
    let long = has(&["长任务", "长时间", "long", "慢一点"]);
    if has(&["失败", "故障", "failure", "fail", "error fixture"]) {
        return Ok((Intent::Fail, "这是一次可恢复的失败演示。我会先输出部分内容，再返回 fixture 错误；模块与已有文件会保留。".into(), long));
    }
    if has(&[
        "修改",
        "改写",
        "更新文档",
        "写入",
        "edit",
        "rewrite",
        "modify",
        "update document",
    ]) {
        let document = prompt
            .context
            .module_id
            .as_ref()
            .and_then(|id| {
                prompt
                    .modules
                    .iter()
                    .find(|module| &module.id == id && module.module_type == ModuleType::Document)
            })
            .or_else(|| {
                prompt
                    .modules
                    .iter()
                    .find(|module| module.module_type == ModuleType::Document)
            })
            .ok_or("请先创建一个文档模块，然后再请求修改")?;
        if document.revision.is_none() {
            return Err("文档无法读取，请先恢复文件再请求修改".into());
        }
        return Ok((Intent::Write(document.clone()), format!("我会为《{}》生成一份修改预览。你可以检查原文与新文的差异；只有明确批准后 Host 才会写入本地文件。", document.title), long));
    }
    let kind = if has(&["计划", "规划", "安排", "planner", "schedule", "plan"]) {
        Some(ModuleType::Planner)
    } else if has(&["看板", "dashboard", "数据概览"]) {
        Some(ModuleType::Dashboard)
    } else if has(&["文档", "笔记", "document", "note", "markdown"]) {
        Some(ModuleType::Document)
    } else if has(&["对话模块", "conversation"]) {
        Some(ModuleType::Conversation)
    } else {
        None
    };
    if let Some(kind) = kind {
        let noun = match kind {
            ModuleType::Planner => "规划",
            ModuleType::Document => "文档",
            ModuleType::Dashboard => "看板",
            ModuleType::Conversation => "对话",
        };
        return Ok((Intent::Create(kind), format!("我会创建一个{noun}模块，并准备可操作的示例内容。接下来会显示模块提议，批准后它会加入画布。你可以拖动、缩放，或双击标题进入聚焦模式。"), long));
    }
    Ok((Intent::Chat, "当前使用本地 Mock Agent，正在验证工作区交互流程。你可以试试「帮我做本周计划」「创建一个文档」「修改文档」「创建数据看板」「模拟失败」或「执行一个长任务」。所有模块提议与文档修改都会交给 Host 审批；本轮不访问网络，也不执行终端。".into(), long))
}
