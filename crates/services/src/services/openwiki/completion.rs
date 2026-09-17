//! Writer operation proof reconstructed from existing Native Audit streams.
//! Bootstrap and ordinary Sync share closure/identity checks, not operation policy.
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

use anyhow::{Context, ensure};
use executors::runtime::{NativeAuditChannel, NativeAuditDirection, NativeAuditReader};
use serde_json::Value;
use sha2::{Digest, Sha256};

fn fingerprint(value: &Value) -> [u8; 32] {
    fn canonical(value: &Value) -> Value {
        match value {
            Value::Object(map) => {
                let sorted: std::collections::BTreeMap<_, _> = map.iter().collect();
                Value::Object(
                    sorted
                        .into_iter()
                        .map(|(key, value)| (key.clone(), canonical(value)))
                        .collect(),
                )
            }
            Value::Array(items) => Value::Array(items.iter().map(canonical).collect()),
            value => value.clone(),
        }
    }
    Sha256::digest(serde_json::to_vec(&canonical(value)).expect("JSON value serialises")).into()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriterPhase {
    Generate,
    Refine,
    Sync,
}

#[derive(Debug)]
struct Operation {
    id: String,
    mode: String,
}

#[derive(Debug)]
pub struct PhaseCompletionProof {
    phase: WriterPhase,
    active: Option<Operation>,
    run_ids: HashSet<String>,
    initialised: bool,
    completed_updates: usize,
    completed_noops: usize,
    pending: HashMap<String, (String, [u8; 32])>,
    completed_calls: HashMap<String, [u8; 32]>,
}

impl PhaseCompletionProof {
    pub fn new(phase: WriterPhase) -> Self {
        Self {
            phase,
            active: None,
            run_ids: HashSet::new(),
            initialised: false,
            completed_updates: 0,
            completed_noops: 0,
            pending: HashMap::new(),
            completed_calls: HashMap::new(),
        }
    }

    /// Attempts must be supplied in durable attempt_number order. A crashed
    /// attempt cannot be forgotten, and cross-attempt operation recovery is not
    /// implemented. An earlier closed, side-effect-free attempt is harmless.
    pub fn replay_attempt(
        &mut self,
        audit: &NativeAuditReader,
        root: &Path,
    ) -> anyhow::Result<bool> {
        self.ensure_closed()?;
        self.completed_calls.clear(); // call IDs are scoped to a provider attempt
        let mut thread: Option<String> = None;
        for frame in audit.frames() {
            if frame.direction != NativeAuditDirection::Output {
                continue;
            }
            let bytes = frame.payload_bytes()?;
            // stderr is also losslessly audited, but is not the app-server wire.
            if frame.channel == NativeAuditChannel::Stderr {
                continue;
            }
            ensure!(
                frame.channel == NativeAuditChannel::Stdout,
                "Unsupported Codex output audit channel"
            );
            let value: Value = serde_json::from_slice(&bytes)
                .context("Invalid Codex protocol frame in Bootstrap audit")?;
            // thread/start and thread/resume RPC responses bind the root for this
            // attempt. Do not use the Session's latest provider ID for old audits.
            if value.get("id").is_some()
                && let Some(id) = value.pointer("/result/thread/id").and_then(Value::as_str)
            {
                if let Some(existing) = &thread {
                    ensure!(existing == id, "Bootstrap attempt changed root thread");
                } else {
                    ensure!(
                        value.pointer("/result/cwd").and_then(Value::as_str)
                            == Some(audit.manifest().workspace_path.as_str()),
                        "Bootstrap root thread cwd does not match Native Audit"
                    );
                    thread = Some(id.to_owned());
                }
            }
            self.observe_frame(&value, thread.as_deref(), root)?;
        }
        self.ensure_closed()?;
        Ok(thread.is_some())
    }

    pub fn ensure_closed(&self) -> anyhow::Result<()> {
        ensure!(
            self.pending.is_empty(),
            "Bootstrap has unresolved OpenWiki MCP calls: {:?}",
            self.pending.keys()
        );
        ensure!(
            self.active.is_none(),
            "Bootstrap has an unfinished OpenWiki run: {:?}",
            self.active
        );
        Ok(())
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        self.ensure_closed()?;
        ensure!(
            self.phase != WriterPhase::Generate || self.initialised,
            "Generate has no completed init from this phase"
        );
        ensure!(
            self.phase != WriterPhase::Sync
                || self.completed_updates > 0
                || self.completed_noops > 0,
            "Sync has no completed update or audited public begin-noop"
        );
        Ok(())
    }

    pub fn has_completed_update(&self) -> bool {
        self.completed_updates > 0
    }

    fn observe_frame(
        &mut self,
        value: &Value,
        thread: Option<&str>,
        root: &Path,
    ) -> anyhow::Result<()> {
        let method = value["method"].as_str().unwrap_or_default();
        let item = &value["params"]["item"];
        if item["type"] != "mcpToolCall" || item["server"] != "openwiki" {
            return Ok(());
        }
        ensure!(
            thread.is_some() && value["params"]["threadId"].as_str() == thread,
            "OpenWiki writer call is not from the authorised root thread"
        );
        ensure!(
            matches!(method, "item/started" | "item/completed"),
            "Unsupported OpenWiki call event"
        );
        let id = item["id"]
            .as_str()
            .filter(|id| !id.is_empty())
            .context("OpenWiki call ID missing")?;
        let tool = item["tool"]
            .as_str()
            .context("OpenWiki tool name missing")?;
        let args = &item["arguments"];
        ensure!(
            matches!(
                tool,
                "openwiki_begin"
                    | "openwiki_submit_plan"
                    | "openwiki_next_page"
                    | "openwiki_inspect_page_claims"
                    | "openwiki_submit_page"
                    | "openwiki_finish"
            ),
            "Unsupported OpenWiki tool {tool}"
        );
        if method == "item/started" {
            ensure!(
                !self.completed_calls.contains_key(id),
                "OpenWiki call restarted after completion"
            );
            if let Some(existing) = self.pending.get(id) {
                ensure!(
                    existing == &(tool.to_owned(), fingerprint(args)),
                    "Conflicting duplicate OpenWiki start"
                );
                return Ok(());
            }
            ensure!(
                self.pending.is_empty(),
                "Overlapping OpenWiki calls are not supported"
            );
            self.pending
                .insert(id.to_owned(), (tool.to_owned(), fingerprint(args)));
            return Ok(());
        }
        if let Some(previous) = self.completed_calls.get(id) {
            ensure!(
                *previous == fingerprint(item),
                "Conflicting duplicate OpenWiki completion"
            );
            return Ok(());
        }
        let started = self
            .pending
            .remove(id)
            .context("OpenWiki completion has no audited start")?;
        ensure!(
            started == (tool.to_owned(), fingerprint(args)),
            "OpenWiki call arguments changed"
        );
        self.completed_calls
            .insert(id.to_owned(), fingerprint(item));
        let failed = item["status"] != "completed"
            || !item["error"].is_null()
            || item["result"]["isError"] == true;
        if tool == "openwiki_begin" {
            // A failed begin can create durable state before returning an error.
            // There is no supported result proving that it was side-effect-free.
            ensure!(!failed, "OpenWiki begin failed with unknown side effects");
            return self.begin(args, &item["result"], root);
        }
        let active = self
            .active
            .as_ref()
            .context("OpenWiki call outside an active owned run")?;
        ensure!(
            args["runId"].as_str() == Some(active.id.as_str()),
            "OpenWiki run ID mismatch"
        );
        if failed {
            // Page/finish validation errors are recoverable within this same run.
            // The run stays active until a later verified finish.
            return Ok(());
        }
        if tool == "openwiki_finish" {
            let data = item["result"]
                .get("structuredContent")
                .unwrap_or(&item["result"]);
            ensure!(
                matches!(data.get("sourceChanged"), None | Some(Value::Bool(false))),
                "Integrated source changed during OpenWiki run"
            );
            ensure!(
                data["status"] == "complete",
                "OpenWiki finish did not prove completion"
            );
            let operation = self.active.take().unwrap();
            if operation.mode == "init" {
                self.initialised = true;
            } else {
                self.completed_updates += 1;
            }
        }
        Ok(())
    }

    fn begin(&mut self, args: &Value, result: &Value, root: &Path) -> anyhow::Result<()> {
        let data = result.get("structuredContent").unwrap_or(result);
        let expected = root.canonicalize().context("Bootstrap root unavailable")?;
        for value in [&args["root"], &data["root"]] {
            let path = Path::new(value.as_str().context("OpenWiki begin root missing")?);
            ensure!(
                path.is_absolute() && path.canonicalize()? == expected,
                "OpenWiki repository root mismatch"
            );
        }
        let mode = args["mode"]
            .as_str()
            .context("OpenWiki begin mode missing")?;
        ensure!(
            data["mode"].as_str() == Some(mode),
            "OpenWiki begin response mode mismatch"
        );
        ensure!(
            matches!(mode, "init" | "update"),
            "Unknown OpenWiki begin mode"
        );
        if mode == "update" {
            ensure!(
                self.phase == WriterPhase::Sync || args["force"] == true,
                "Bootstrap corrections require update + force=true"
            );
            ensure!(
                self.phase != WriterPhase::Generate || self.initialised,
                "Generate update preceded init completion"
            );
        } else {
            ensure!(
                self.phase == WriterPhase::Generate && !self.initialised,
                "Bootstrap cannot reinitialise Wiki"
            );
        }
        // 0.5.1's public no-op response has no runId or resumed flag. It is
        // successful only for an ordinary unforced update with no open run.
        if data["status"] == "noop" {
            ensure!(
                self.phase == WriterPhase::Sync
                    && mode == "update"
                    && args["force"] != true
                    && self.active.is_none()
                    && matches!(data.get("sourceChanged"), None | Some(Value::Bool(false))),
                "OpenWiki no-op cannot close an active or forced operation"
            );
            self.completed_noops += 1;
            return Ok(());
        }
        ensure!(
            data["status"] == "active",
            "Bootstrap begin did not start an authoring run"
        );
        let id = data["runId"]
            .as_str()
            .context("OpenWiki begin run ID missing")?;
        uuid::Uuid::parse_str(id).context("Invalid OpenWiki run ID")?;
        let resumed = data["resumed"]
            .as_bool()
            .context("OpenWiki begin resumed flag missing")?;
        if let Some(active) = &self.active {
            ensure!(
                resumed && active.id == id && active.mode == mode,
                "OpenWiki abandoned or replaced an active run"
            );
        } else {
            ensure!(
                !resumed && self.run_ids.insert(id.to_owned()),
                "OpenWiki run was not started in this phase or reused a closed ID"
            );
            self.active = Some(Operation {
                id: id.to_owned(),
                mode: mode.to_owned(),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use uuid::Uuid;

    use super::*;

    fn frames(tool: &str, args: Value, result: Value) -> Vec<Value> {
        let id = Uuid::new_v4().to_string();
        ["item/started", "item/completed"]
            .into_iter()
            .map(|method| {
                json!({
                    "method":method,"params":{"threadId":"root","item":{
                        "id":id,"type":"mcpToolCall","server":"openwiki","tool":tool,
                        "arguments":args,"result":result,"status":"completed","error":null
                    }}
                })
            })
            .collect()
    }

    fn begin(root: &Path, mode: &str, id: Uuid) -> Vec<Value> {
        frames(
            "openwiki_begin",
            json!({"root":root,"mode":mode,"force":true}),
            json!({"structuredContent":{"root":root,"mode":mode,"runId":id,"status":"active","resumed":false}}),
        )
    }

    fn finish(id: Uuid) -> Vec<Value> {
        frames(
            "openwiki_finish",
            json!({"runId":id}),
            json!({"structuredContent":{"status":"complete"}}),
        )
    }

    fn feed(
        proof: &mut PhaseCompletionProof,
        root: &Path,
        frames: Vec<Value>,
    ) -> anyhow::Result<()> {
        for frame in frames {
            proof.observe_frame(&frame, Some("root"), root)?;
        }
        Ok(())
    }

    #[test]
    fn generate_init_then_multiple_self_corrections_and_refine_without_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let mut proof = PhaseCompletionProof::new(WriterPhase::Generate);
        let init = Uuid::new_v4();
        feed(&mut proof, tmp.path(), begin(tmp.path(), "init", init)).unwrap();
        assert!(proof.validate().is_err());
        feed(&mut proof, tmp.path(), finish(init)).unwrap();
        proof.validate().unwrap();
        for _ in 0..3 {
            let update = Uuid::new_v4();
            feed(&mut proof, tmp.path(), begin(tmp.path(), "update", update)).unwrap();
            feed(&mut proof, tmp.path(), finish(update)).unwrap();
        }
        proof.validate().unwrap();
        assert!(proof.has_completed_update());
        let mut refine = PhaseCompletionProof::new(WriterPhase::Refine);
        refine.validate().unwrap(); // semantic report is validated separately
        for _ in 0..2 {
            let id = Uuid::new_v4();
            feed(&mut refine, tmp.path(), begin(tmp.path(), "update", id)).unwrap();
            feed(&mut refine, tmp.path(), finish(id)).unwrap();
        }
        refine.validate().unwrap();
    }

    #[test]
    fn phase_modes_and_force_are_enforced() {
        let tmp = tempfile::tempdir().unwrap();
        for (phase, mode, force) in [
            (WriterPhase::Generate, "update", true),
            (WriterPhase::Refine, "init", true),
            (WriterPhase::Refine, "update", false),
        ] {
            let mut proof = PhaseCompletionProof::new(phase);
            let mut input = begin(tmp.path(), mode, Uuid::new_v4());
            for frame in &mut input {
                frame["params"]["item"]["arguments"]["force"] = json!(force);
            }
            assert!(feed(&mut proof, tmp.path(), input).is_err());
        }
        let mut proof = PhaseCompletionProof::new(WriterPhase::Generate);
        assert!(proof.validate().is_err());
        let id = Uuid::new_v4();
        feed(&mut proof, tmp.path(), begin(tmp.path(), "init", id)).unwrap();
        feed(&mut proof, tmp.path(), finish(id)).unwrap();
        assert!(
            feed(
                &mut proof,
                tmp.path(),
                begin(tmp.path(), "init", Uuid::new_v4())
            )
            .is_err()
        );
    }

    #[test]
    fn failed_begin_and_unfinished_followup_cannot_hide_behind_prior_success() {
        let tmp = tempfile::tempdir().unwrap();
        let mut proof = PhaseCompletionProof::new(WriterPhase::Generate);
        let id = Uuid::new_v4();
        feed(&mut proof, tmp.path(), begin(tmp.path(), "init", id)).unwrap();
        feed(&mut proof, tmp.path(), finish(id)).unwrap();
        let mut next = begin(tmp.path(), "update", Uuid::new_v4());
        next[1]["params"]["item"]["result"] = json!({"isError":true});
        assert!(feed(&mut proof, tmp.path(), next).is_err());

        let mut proof = PhaseCompletionProof::new(WriterPhase::Refine);
        let mut next = begin(tmp.path(), "update", Uuid::new_v4());
        next.pop();
        feed(&mut proof, tmp.path(), next).unwrap();
        assert!(proof.validate().is_err());
    }

    #[test]
    fn same_run_validation_repair_and_resumed_begin_are_allowed() {
        let tmp = tempfile::tempdir().unwrap();
        let mut proof = PhaseCompletionProof::new(WriterPhase::Generate);
        let id = Uuid::new_v4();
        feed(&mut proof, tmp.path(), begin(tmp.path(), "init", id)).unwrap();
        for tool in ["openwiki_submit_page", "openwiki_finish"] {
            feed(
                &mut proof,
                tmp.path(),
                frames(tool, json!({"runId":id}), json!({"isError":true})),
            )
            .unwrap();
            assert!(proof.validate().is_err());
        }
        let mut resume = begin(tmp.path(), "init", id);
        for frame in &mut resume {
            frame["params"]["item"]["result"]["structuredContent"]["resumed"] = json!(true);
        }
        feed(&mut proof, tmp.path(), resume).unwrap();
        feed(&mut proof, tmp.path(), finish(id)).unwrap();
        proof.validate().unwrap();
    }

    #[test]
    fn unknown_resumed_run_wrong_root_and_child_writer_are_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        for mutation in 0..4 {
            let mut proof = PhaseCompletionProof::new(WriterPhase::Generate);
            let mut input = begin(tmp.path(), "init", Uuid::new_v4());
            for frame in &mut input {
                match mutation {
                    0 => {
                        frame["params"]["item"]["result"]["structuredContent"]["resumed"] =
                            json!(true)
                    }
                    1 => {
                        frame["params"]["item"]["result"]["structuredContent"]["root"] =
                            json!(other.path())
                    }
                    2 => frame["params"]["threadId"] = json!("child"),
                    _ => {
                        frame["params"]["item"]["result"]["structuredContent"]["mode"] =
                            json!("update")
                    }
                }
            }
            assert!(feed(&mut proof, tmp.path(), input).is_err());
        }
    }

    #[test]
    fn unclosed_run_wrong_finish_and_source_drift_fail_closed() {
        let tmp = tempfile::tempdir().unwrap();
        for mutation in 0..3 {
            let mut proof = PhaseCompletionProof::new(WriterPhase::Generate);
            let id = Uuid::new_v4();
            feed(&mut proof, tmp.path(), begin(tmp.path(), "init", id)).unwrap();
            let mut input = match mutation {
                0 => begin(tmp.path(), "init", Uuid::new_v4()),
                1 => finish(Uuid::new_v4()),
                _ => finish(id),
            };
            if mutation == 2 {
                input[1]["params"]["item"]["result"]["structuredContent"]["sourceChanged"] =
                    json!(true);
            }
            assert!(feed(&mut proof, tmp.path(), input).is_err());
        }
    }

    #[test]
    fn duplicate_completions_are_idempotent_but_conflicts_and_missing_start_fail() {
        assert_eq!(
            fingerprint(&serde_json::from_str::<Value>(r#"{"a":1,"b":{"c":2,"d":3}}"#).unwrap()),
            fingerprint(&serde_json::from_str::<Value>(r#"{"b":{"d":3,"c":2},"a":1}"#).unwrap())
        );
        let tmp = tempfile::tempdir().unwrap();
        let mut proof = PhaseCompletionProof::new(WriterPhase::Generate);
        let id = Uuid::new_v4();
        feed(&mut proof, tmp.path(), begin(tmp.path(), "init", id)).unwrap();
        let input = finish(id);
        feed(&mut proof, tmp.path(), input.clone()).unwrap();
        feed(&mut proof, tmp.path(), vec![input[1].clone()]).unwrap();
        let mut conflict = input[1].clone();
        conflict["params"]["item"]["result"] = json!({"status":"different"});
        assert!(feed(&mut proof, tmp.path(), vec![conflict]).is_err());
        let mut proof = PhaseCompletionProof::new(WriterPhase::Generate);
        assert!(feed(&mut proof, tmp.path(), vec![input[1].clone()]).is_err());
    }

    fn audited(
        root: &Path,
        frames: Vec<Value>,
        number: u32,
    ) -> (tempfile::TempDir, NativeAuditReader) {
        use executors::runtime::{NativeAuditMetadata, NativeAuditWriter};
        let temp = tempfile::tempdir().unwrap();
        let mut writer = NativeAuditWriter::create_in(
            temp.path(),
            NativeAuditMetadata {
                session_id: Uuid::new_v4(),
                agent_run_id: Uuid::new_v4(),
                turn_id: Uuid::new_v4(),
                run_attempt_id: Uuid::new_v4(),
                run_attempt_number: number,
                provider_id: "codex".into(),
                runtime_profile_id: "test".into(),
                workspace_path: root.to_string_lossy().into_owned(),
                runtime_version: None,
                protocol_version: None,
                adapter_version: "test".into(),
                mapper_version: "test".into(),
                created_at: chrono::Utc::now(),
            },
        )
        .unwrap();
        let header = json!({"id":3,"result":{"thread":{"id":"root"},"cwd":root}});
        for frame in std::iter::once(header).chain(frames) {
            writer
                .append_bytes(
                    NativeAuditDirection::Output,
                    NativeAuditChannel::Stdout,
                    "application/json",
                    Uuid::new_v4(),
                    &serde_json::to_vec(&frame).unwrap(),
                    None,
                )
                .unwrap();
        }
        let manifest = writer.close().unwrap();
        let path = temp.path().join(manifest.manifest_relative_path);
        let reader = NativeAuditReader::read(path.parent().unwrap()).unwrap();
        (temp, reader)
    }

    #[test]
    fn real_native_audit_replay_includes_prior_attempt_and_recovers_no_side_effect_failure() {
        let tmp = tempfile::tempdir().unwrap();
        let id = Uuid::new_v4();
        let (_first, first) = audited(tmp.path(), vec![], 1);
        let sequence = begin(tmp.path(), "init", id)
            .into_iter()
            .chain(finish(id))
            .collect();
        let (_second, second) = audited(tmp.path(), sequence, 2);
        let mut proof = PhaseCompletionProof::new(WriterPhase::Generate);
        proof.replay_attempt(&first, tmp.path()).unwrap();
        proof.replay_attempt(&second, tmp.path()).unwrap();
        proof.validate().unwrap();
        let (_bad, bad) = audited(tmp.path(), begin(tmp.path(), "init", Uuid::new_v4()), 1);
        let mut proof = PhaseCompletionProof::new(WriterPhase::Generate);
        assert!(proof.replay_attempt(&bad, tmp.path()).is_err());
        assert!(proof.replay_attempt(&second, tmp.path()).is_err());
    }

    #[test]
    fn sync_accepts_unforced_updates_multiple_operations_and_public_noop() {
        let tmp = tempfile::tempdir().unwrap();
        let mut proof = PhaseCompletionProof::new(WriterPhase::Sync);
        assert!(proof.validate().is_err());
        for _ in 0..2 {
            let id = Uuid::new_v4();
            let mut input = begin(tmp.path(), "update", id);
            for frame in &mut input {
                frame["params"]["item"]["arguments"]
                    .as_object_mut()
                    .unwrap()
                    .remove("force");
            }
            feed(&mut proof, tmp.path(), input).unwrap();
            assert!(proof.validate().is_err());
            feed(&mut proof, tmp.path(), finish(id)).unwrap();
            proof.validate().unwrap();
        }
        let mut noop = PhaseCompletionProof::new(WriterPhase::Sync);
        feed(
            &mut noop,
            tmp.path(),
            frames(
                "openwiki_begin",
                json!({"root":tmp.path(),"mode":"update"}),
                json!({"structuredContent":{"root":tmp.path(),"mode":"update","status":"noop"}}),
            ),
        )
        .unwrap();
        noop.validate().unwrap();
        assert!(!noop.has_completed_update());
    }

    #[test]
    fn sync_cannot_hide_unfinished_operations_or_unaudited_calls_behind_noop() {
        let tmp = tempfile::tempdir().unwrap();
        for mutation in [
            "active", "force", "init", "root", "mode", "source", "child", "failed",
        ] {
            let mut proof = PhaseCompletionProof::new(WriterPhase::Sync);
            let mut input = frames(
                "openwiki_begin",
                json!({"root":tmp.path(),"mode":"update"}),
                json!({"structuredContent":{"root":tmp.path(),"mode":"update","status":"noop"}}),
            );
            if mutation == "active" {
                feed(
                    &mut proof,
                    tmp.path(),
                    begin(tmp.path(), "update", Uuid::new_v4()),
                )
                .unwrap();
            }
            for frame in &mut input {
                let item = &mut frame["params"]["item"];
                match mutation {
                    "force" => item["arguments"]["force"] = json!(true),
                    "init" => {
                        item["arguments"]["mode"] = json!("init");
                        item["result"]["structuredContent"]["mode"] = json!("init");
                    }
                    "root" => item["result"]["structuredContent"]["root"] = json!("/unknown"),
                    "mode" => item["result"]["structuredContent"]["mode"] = json!("init"),
                    "source" => item["result"]["structuredContent"]["sourceChanged"] = json!(true),
                    "failed" => item["result"]["isError"] = json!(true),
                    "child" => frame["params"]["threadId"] = json!("child"),
                    _ => {}
                }
            }
            assert!(feed(&mut proof, tmp.path(), input).is_err(), "{mutation}");
        }
        let mut proof = PhaseCompletionProof::new(WriterPhase::Sync);
        let (_tmp, audit) = audited(tmp.path(), vec![], 1);
        proof.replay_attempt(&audit, tmp.path()).unwrap();
        assert!(proof.validate().is_err()); // prose/custom stdio bridge is not MCP evidence
    }
}
