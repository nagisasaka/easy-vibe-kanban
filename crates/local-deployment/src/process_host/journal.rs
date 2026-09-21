//! Attempt-owned, grouped replay evidence; not a second raw provider audit.
//! Payloads live on disk. Only bounded pending/live buffers and one offset per
//! committed batch remain in memory. Sequence visibility follows sync_data.
use std::{collections::VecDeque, io::SeekFrom, path::Path};

use executors::runtime::AgentLiveEventPayload;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::{
    fs::File,
    io::{AsyncBufReadExt, AsyncReadExt, AsyncSeekExt, AsyncWriteExt, BufReader},
};
use uuid::Uuid;

use super::{HostEvent, HostEventPayload, ProcessHostError};

pub(super) const PAGE_EVENTS: usize = 128;
pub(super) const PAGE_BYTES: usize = 4 * 1024 * 1024;
const FLUSH_BYTES: usize = 256 * 1024;
const MAX_EVENT_BYTES: usize = 8 * 1024 * 1024;
const MAX_BATCH_BYTES: usize = MAX_EVENT_BYTES + FLUSH_BYTES + 64 * 1024;
const LIVE_EVENTS: usize = 256;
const LIVE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
struct Batch {
    version: u16,
    attempt: Uuid,
    host: Uuid,
    checksum: String,
    events: Vec<HostEvent>,
}

struct Offset {
    first: u64,
    last: u64,
    start: u64,
    bytes: usize,
}

pub(crate) struct HostJournal {
    file: File,
    attempt: Uuid,
    host: Uuid,
    offsets: Vec<Offset>,
    end: u64,
    pending: Vec<HostEvent>,
    pending_bytes: usize,
    live: VecDeque<(HostEvent, usize)>,
    live_bytes: usize,
    committed: u64,
    failure: Option<String>,
}

fn protocol(message: impl Into<String>) -> ProcessHostError {
    ProcessHostError::Protocol(message.into())
}

fn encode(value: &impl Serialize) -> Result<Vec<u8>, ProcessHostError> {
    serde_json::to_vec(value).map_err(|error| protocol(error.to_string()))
}

pub(crate) fn identity(event: &HostEvent, attempt: Uuid) -> bool {
    match &event.payload {
        HostEventPayload::Started {
            audit_manifest,
            canonical_input_ref,
            ..
        } => audit_manifest.run_attempt_id == attempt && canonical_input_ref.stream_id == attempt,
        HostEventPayload::Terminal { audit_manifest, .. } => {
            audit_manifest.run_attempt_id == attempt
        }
        HostEventPayload::Mapped { event, native_ref } => {
            event.run_attempt_id == attempt && native_ref.stream_id == attempt
        }
        HostEventPayload::Projected {
            durable_events,
            live_events,
            native_ref,
        } => {
            native_ref.stream_id == attempt
                && durable_events.iter().all(|e| e.run_attempt_id == attempt)
                && live_events.iter().all(|e| e.run_attempt_id == attempt)
        }
    }
}

fn decode(
    bytes: &[u8],
    attempt: Uuid,
    host: Uuid,
    previous: u64,
) -> Result<Vec<HostEvent>, ProcessHostError> {
    let batch: Batch = serde_json::from_slice(bytes)
        .map_err(|error| protocol(format!("corrupt host journal batch: {error}")))?;
    if batch.version != 1
        || batch.attempt != attempt
        || batch.host != host
        || batch.events.is_empty()
        || batch.events.len() > PAGE_EVENTS
    {
        return Err(protocol("host journal version/identity/count mismatch"));
    }
    let actual = format!("{:x}", Sha256::digest(encode(&batch.events)?));
    if actual != batch.checksum {
        return Err(protocol("host journal checksum mismatch"));
    }
    for (index, event) in batch.events.iter().enumerate() {
        if event.sequence != previous + index as u64 + 1 || !identity(event, attempt) {
            return Err(protocol("host journal sequence gap or foreign attempt"));
        }
    }
    Ok(batch.events)
}

impl HostJournal {
    pub async fn create(path: &Path, attempt: Uuid, host: Uuid) -> Result<Self, ProcessHostError> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let file = tokio::fs::OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(path)
            .await?;
        file.sync_all().await?;
        Ok(Self {
            file,
            attempt,
            host,
            offsets: Vec::new(),
            end: 0,
            pending: Vec::new(),
            pending_bytes: 0,
            live: VecDeque::new(),
            live_bytes: 0,
            committed: 0,
            failure: None,
        })
    }

    fn healthy(&self) -> Result<(), ProcessHostError> {
        self.failure
            .as_ref()
            .map_or(Ok(()), |message| Err(protocol(message.clone())))
    }

    pub fn committed_sequence(&self) -> u64 {
        self.committed
    }
    pub fn next_sequence(&self) -> u64 {
        self.committed + self.pending.len() as u64 + 1
    }

    pub async fn append(&mut self, event: HostEvent) -> Result<(), ProcessHostError> {
        self.healthy()?;
        if event.sequence != self.next_sequence() || !identity(&event, self.attempt) {
            self.failure = Some("invalid host journal append identity/sequence".into());
            return self.healthy();
        }
        let flush_now = matches!(
            event.payload,
            HostEventPayload::Started { .. } | HostEventPayload::Terminal { .. }
        );
        let mut durable = event.clone();
        if let HostEventPayload::Projected { live_events, .. } = &mut durable.payload {
            // Usage is a persisted latest-value plane, unlike transient text.
            live_events
                .retain(|e| matches!(e.payload, AgentLiveEventPayload::TokenUsageSnapshot { .. }));
        }
        let bytes = encode(&durable)?.len();
        if bytes > MAX_EVENT_BYTES {
            self.failure = Some(format!(
                "host semantic event exceeds {MAX_EVENT_BYTES} byte limit; inspect Native Audit"
            ));
            return self.healthy();
        }
        if !self.pending.is_empty() && self.pending_bytes + bytes > FLUSH_BYTES {
            self.flush().await?;
        }
        let live_bytes = encode(&event)?.len();
        if live_bytes <= LIVE_BYTES {
            while self.live.len() >= LIVE_EVENTS || self.live_bytes + live_bytes > LIVE_BYTES {
                if let Some((_, size)) = self.live.pop_front() {
                    self.live_bytes -= size;
                } else {
                    break;
                }
            }
            self.live_bytes += live_bytes;
            self.live.push_back((event, live_bytes));
        }
        self.pending.push(durable);
        self.pending_bytes += bytes;
        if flush_now || self.pending.len() >= PAGE_EVENTS || self.pending_bytes >= FLUSH_BYTES {
            self.flush().await?;
        }
        Ok(())
    }

    pub async fn flush(&mut self) -> Result<(), ProcessHostError> {
        self.healthy()?;
        if self.pending.is_empty() {
            return Ok(());
        }
        let checksum = format!("{:x}", Sha256::digest(encode(&self.pending)?));
        let batch = Batch {
            version: 1,
            attempt: self.attempt,
            host: self.host,
            checksum,
            events: self.pending.clone(),
        };
        let mut bytes = encode(&batch)?;
        bytes.push(b'\n');
        if bytes.len() > MAX_BATCH_BYTES {
            return Err(protocol("host journal batch exceeds byte limit"));
        }
        let result = async {
            self.file.seek(SeekFrom::Start(self.end)).await?;
            self.file.write_all(&bytes).await?;
            self.file.flush().await?;
            self.file.sync_data().await
        }
        .await;
        if let Err(error) = result {
            self.failure = Some(format!("host journal commit failed: {error}"));
            return Err(ProcessHostError::Io(error));
        }
        let last = self.pending.last().expect("nonempty batch").sequence;
        self.offsets.push(Offset {
            first: self.committed + 1,
            last,
            start: self.end,
            bytes: bytes.len(),
        });
        self.end += bytes.len() as u64;
        self.committed = last;
        self.pending.clear();
        self.pending_bytes = 0;
        Ok(())
    }

    pub async fn read_after(&mut self, after: u64) -> Result<Vec<HostEvent>, ProcessHostError> {
        self.healthy()?;
        if after > self.committed {
            return Err(protocol("host cursor is beyond committed journal"));
        }
        let mut output = Vec::new();
        let mut size = 0;
        let start = self.offsets.partition_point(|offset| offset.last <= after);
        for offset in &self.offsets[start..] {
            self.file.seek(SeekFrom::Start(offset.start)).await?;
            let mut bytes = vec![0; offset.bytes];
            self.file.read_exact(&mut bytes).await?;
            for mut event in decode(&bytes, self.attempt, self.host, offset.first - 1)? {
                if event.sequence <= after {
                    continue;
                }
                if let Some((live, _)) = self
                    .live
                    .iter()
                    .find(|(live, _)| live.sequence == event.sequence)
                {
                    event = live.clone();
                }
                let bytes = encode(&event)?.len();
                // A single large event is explicit and bounded by MAX_EVENT_BYTES
                // (or LIVE_BYTES); it is never split or silently truncated.
                if output.len() >= PAGE_EVENTS || (!output.is_empty() && size + bytes > PAGE_BYTES)
                {
                    return Ok(output);
                }
                size += bytes;
                output.push(event);
            }
        }
        Ok(output)
    }
}

/// A reader for a *confirmed dead* owner. Validate the whole file before any
/// replay to avoid committing a terminal fact ahead of later corruption.
pub(crate) struct HostJournalReplay {
    reader: BufReader<File>,
    attempt: Uuid,
    host: Uuid,
    through: u64,
    after: u64,
    pending: VecDeque<HostEvent>,
    pub incomplete_tail: bool,
    pub started: Option<HostEvent>,
    pub last_sequence: u64,
    pub terminal: Option<HostEvent>,
}

impl HostJournalReplay {
    pub async fn verify_audit(
        &mut self,
        directory: &Path,
        session: Uuid,
        run: Uuid,
    ) -> Result<(), ProcessHostError> {
        let mut audit = executors::runtime::NativeAuditEvidenceReader::open(
            directory,
            session,
            run,
            self.attempt,
        )
        .map_err(|error| ProcessHostError::Audit(error.to_string()))?;
        let saved_after = self.after;
        self.after = 0;
        let mut terminal_manifest = None;
        loop {
            let page = self.next_page().await?;
            if page.is_empty() {
                break;
            }
            for event in page {
                let reference = match event.payload {
                    HostEventPayload::Started {
                        canonical_input_ref,
                        audit_manifest,
                        ..
                    } => {
                        if audit_manifest.session_id != session
                            || audit_manifest.agent_run_id != run
                        {
                            return Err(protocol("journal audit identity mismatch"));
                        }
                        Some(canonical_input_ref)
                    }
                    HostEventPayload::Mapped { native_ref, .. }
                    | HostEventPayload::Projected { native_ref, .. } => Some(native_ref),
                    HostEventPayload::Terminal {
                        audit_manifest,
                        status,
                        ..
                    } => {
                        if status == executors::runtime::AgentRunStatus::Succeeded
                            && !matches!(
                                audit_manifest.integrity_status,
                                executors::runtime::NativeAuditIntegrityStatus::Complete
                                    | executors::runtime::NativeAuditIntegrityStatus::Recovered
                            )
                        {
                            return Err(protocol(
                                "successful terminal requires complete Native Audit",
                            ));
                        }
                        terminal_manifest = Some(audit_manifest);
                        None
                    }
                };
                if let Some(reference) = reference {
                    audit
                        .verify_reference(&reference)
                        .map_err(|error| ProcessHostError::Audit(error.to_string()))?;
                }
            }
        }
        if let Some(manifest) = terminal_manifest {
            audit
                .verify_terminal(&manifest)
                .map_err(|error| ProcessHostError::Audit(error.to_string()))?;
        }
        self.reader.seek(SeekFrom::Start(0)).await?;
        self.through = 0;
        self.after = saved_after;
        self.pending.clear();
        Ok(())
    }

    pub async fn open(
        path: &Path,
        attempt: Uuid,
        host: Uuid,
        after: u64,
    ) -> Result<Self, ProcessHostError> {
        let mut result = Self {
            // A dead host may have completed write_all but not sync_data. Make
            // complete records durable before permitting a DB cursor to cover
            // them. Never change content or repair an incomplete final record.
            reader: BufReader::new(
                tokio::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(path)
                    .await?,
            ),
            attempt,
            host,
            through: 0,
            after,
            pending: VecDeque::new(),
            incomplete_tail: false,
            started: None,
            last_sequence: 0,
            terminal: None,
        };
        let mut terminal = false;
        while let Some(events) = result.read_batch().await? {
            for event in events {
                if terminal {
                    return Err(protocol("host journal has events after terminal"));
                }
                terminal = matches!(event.payload, HostEventPayload::Terminal { .. });
                if terminal {
                    result.terminal = Some(event.clone());
                }
                if matches!(event.payload, HostEventPayload::Started { .. }) {
                    if result.started.is_some() || event.sequence != 1 {
                        return Err(protocol("invalid repeated/late host Started event"));
                    }
                    result.started = Some(event);
                }
            }
        }
        if terminal && result.incomplete_tail {
            return Err(protocol("uncommitted data after terminal"));
        }
        if after > result.through {
            return Err(protocol("host DB cursor exceeds recoverable journal"));
        }
        result.reader.get_ref().sync_data().await?;
        result.reader.seek(SeekFrom::Start(0)).await?;
        result.last_sequence = result.through;
        result.through = 0;
        Ok(result)
    }

    async fn read_batch(&mut self) -> Result<Option<Vec<HostEvent>>, ProcessHostError> {
        let mut bytes = Vec::new();
        (&mut self.reader)
            .take((MAX_BATCH_BYTES + 1) as u64)
            .read_until(b'\n', &mut bytes)
            .await?;
        if bytes.len() > MAX_BATCH_BYTES {
            return Err(protocol("host journal record exceeds byte limit"));
        }
        if bytes.is_empty() {
            return Ok(None);
        }
        if bytes.last() != Some(&b'\n') {
            self.incomplete_tail = true;
            return Ok(None);
        }
        let events = decode(&bytes, self.attempt, self.host, self.through)?;
        self.through = events.last().expect("validated batch").sequence;
        Ok(Some(events))
    }

    pub async fn next_page(&mut self) -> Result<Vec<HostEvent>, ProcessHostError> {
        let mut events = Vec::new();
        let mut size = 0;
        loop {
            if self.pending.is_empty() {
                match self.read_batch().await? {
                    Some(batch) => self.pending.extend(batch),
                    None => return Ok(events),
                }
            }
            while let Some(next) = self.pending.front() {
                if next.sequence <= self.after {
                    self.pending.pop_front();
                    continue;
                }
                let bytes = encode(next)?.len();
                if events.len() >= PAGE_EVENTS || (!events.is_empty() && size + bytes > PAGE_BYTES)
                {
                    return Ok(events);
                }
                size += bytes;
                events.push(self.pending.pop_front().expect("front present"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use executors::runtime::NativeAuditReference;

    use super::*;

    fn event(sequence: u64, attempt: Uuid) -> HostEvent {
        HostEvent {
            sequence,
            event_id: Uuid::new_v4(),
            timestamp: chrono::Utc::now(),
            payload: HostEventPayload::Projected {
                durable_events: vec![],
                live_events: vec![],
                native_ref: NativeAuditReference {
                    stream_id: attempt,
                    sequence,
                    checksum: Some("audit-checksum".into()),
                },
            },
        }
    }

    fn live(sequence: u64, attempt: Uuid, content: &str) -> HostEvent {
        let mut result = event(sequence, attempt);
        if let HostEventPayload::Projected { live_events, .. } = &mut result.payload {
            live_events.push(executors::runtime::AgentLiveEvent {
                schema_version: 1,
                event_id: Uuid::new_v4(),
                session_id: Uuid::new_v4(),
                agent_run_id: Uuid::new_v4(),
                turn_id: Uuid::new_v4(),
                run_attempt_id: attempt,
                run_attempt_number: 1,
                native_sequence: sequence,
                timestamp: chrono::Utc::now(),
                payload: AgentLiveEventPayload::ThinkingDelta {
                    provider_item_id: "item".into(),
                    delta: content.into(),
                },
            });
        }
        result
    }

    #[tokio::test]
    async fn transient_text_is_bounded_not_durable_but_usage_survives_replay() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("journal");
        let attempt = Uuid::new_v4();
        let host = Uuid::new_v4();
        let mut journal = HostJournal::create(&path, attempt, host).await.unwrap();
        let start = std::time::Instant::now();
        let mut prior_memory = 0;
        for sequence in 1..=1000 {
            let event = live(
                sequence,
                attempt,
                &format!("transient-only-{}", "x".repeat(5000)),
            );
            prior_memory += encode(&event).unwrap().len();
            journal.append(event).await.unwrap();
        }
        let mut usage = live(1001, attempt, "not text");
        if let HostEventPayload::Projected { live_events, .. } = &mut usage.payload {
            live_events[0].payload = AgentLiveEventPayload::TokenUsageSnapshot {
                input_tokens: 1234,
                output_tokens: 56,
                cached_input_tokens: Some(12),
            };
        }
        journal.append(usage).await.unwrap();
        journal.flush().await.unwrap();
        let bytes = tokio::fs::read(&path).await.unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("transient-only"));
        assert!(journal.live_bytes <= LIVE_BYTES);
        assert!(journal.live.len() <= LIVE_EVENTS);
        let mut replay = HostJournalReplay::open(&path, attempt, host, 999)
            .await
            .unwrap();
        let page = replay.next_page().await.unwrap();
        assert_eq!(page.len(), 2);
        let HostEventPayload::Projected { live_events, .. } = &page[0].payload else {
            panic!()
        };
        assert!(live_events.is_empty());
        let HostEventPayload::Projected { live_events, .. } = &page[1].payload else {
            panic!()
        };
        assert!(matches!(
            live_events[0].payload,
            AgentLiveEventPayload::TokenUsageSnapshot {
                input_tokens: 1234,
                ..
            }
        ));
        eprintln!(
            "1000-delta fixture: old retained/Attach bytes={prior_memory}; live bytes={}, journal bytes={}, grouped fsync={}, offset bytes={}, elapsed_ms={}",
            journal.live_bytes,
            bytes.len(),
            journal.offsets.len(),
            journal.offsets.len() * std::mem::size_of::<Offset>(),
            start.elapsed().as_millis()
        );
    }

    #[tokio::test]
    async fn page_byte_limit_and_single_event_hard_limit_are_explicit() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("journal");
        let attempt = Uuid::new_v4();
        let host = Uuid::new_v4();
        let mut journal = HostJournal::create(&path, attempt, host).await.unwrap();
        // Huge checksum strings stand in for large semantic fields, avoiding
        // dependence on a particular provider's payload format.
        for sequence in 1..=3 {
            let mut next = event(sequence, attempt);
            if let HostEventPayload::Projected { native_ref, .. } = &mut next.payload {
                native_ref.checksum = Some("x".repeat(PAGE_BYTES / 2));
            }
            journal.append(next).await.unwrap();
        }
        journal.flush().await.unwrap();
        assert_eq!(journal.read_after(0).await.unwrap().len(), 1);
        let mut next = event(4, attempt);
        if let HostEventPayload::Projected { native_ref, .. } = &mut next.payload {
            native_ref.checksum = Some("x".repeat(MAX_EVENT_BYTES));
        }
        assert!(journal.append(next).await.is_err());
        assert_eq!(journal.committed_sequence(), 3);
    }

    #[tokio::test]
    async fn grouped_sync_bounds_memory_and_does_not_expose_uncommitted_cursor() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("journal");
        let attempt = Uuid::new_v4();
        let host = Uuid::new_v4();
        let mut journal = HostJournal::create(&path, attempt, host).await.unwrap();
        for sequence in 1..=1000 {
            journal.append(event(sequence, attempt)).await.unwrap();
        }
        assert_eq!(journal.committed_sequence(), 896);
        assert_eq!(journal.offsets.len(), 7);
        assert!(journal.live.len() <= LIVE_EVENTS && journal.live_bytes <= LIVE_BYTES);
        assert!(journal.pending.len() < PAGE_EVENTS);
        let page = journal.read_after(0).await.unwrap();
        assert_eq!(page.len(), PAGE_EVENTS);
        assert_eq!(page.last().unwrap().sequence, 128);
        journal.flush().await.unwrap();
        assert_eq!(journal.committed_sequence(), 1000);
        assert_eq!(journal.offsets.len(), 8);
        let mut replay = HostJournalReplay::open(&path, attempt, host, 750)
            .await
            .unwrap();
        let mut count = 0;
        loop {
            let page = replay.next_page().await.unwrap();
            if page.is_empty() {
                break;
            }
            assert!(page.len() <= PAGE_EVENTS);
            count += page.len();
        }
        assert_eq!(count, 250);
    }

    #[tokio::test]
    async fn replay_rejects_corrupt_foreign_and_gapped_batches_but_reports_incomplete_tail() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("journal");
        let attempt = Uuid::new_v4();
        let host = Uuid::new_v4();
        let mut journal = HostJournal::create(&path, attempt, host).await.unwrap();
        journal.append(event(1, attempt)).await.unwrap();
        journal.flush().await.unwrap();
        let original = tokio::fs::read(&path).await.unwrap();
        assert!(
            HostJournalReplay::open(&path, Uuid::new_v4(), host, 0)
                .await
                .is_err()
        );
        assert!(
            HostJournalReplay::open(&path, attempt, Uuid::new_v4(), 0)
                .await
                .is_err()
        );
        assert!(
            HostJournalReplay::open(&path, attempt, host, 2)
                .await
                .is_err()
        );
        let mut partial = original.clone();
        partial.extend(b"{unfinished");
        tokio::fs::write(&path, partial).await.unwrap();
        let mut replay = HostJournalReplay::open(&path, attempt, host, 0)
            .await
            .unwrap();
        assert!(replay.incomplete_tail);
        assert_eq!(replay.next_page().await.unwrap().len(), 1);
        let mut bad = original.clone();
        bad.extend(b"{}\n");
        tokio::fs::write(&path, bad).await.unwrap();
        assert!(
            HostJournalReplay::open(&path, attempt, host, 0)
                .await
                .is_err()
        );
        let mut batch: Batch = serde_json::from_slice(&original).unwrap();
        batch.events[0].sequence = 2;
        // First checksum corruption, then a self-consistent checksum with a gap.
        let mut bad = encode(&batch).unwrap();
        bad.push(b'\n');
        tokio::fs::write(&path, bad).await.unwrap();
        assert!(
            HostJournalReplay::open(&path, attempt, host, 0)
                .await
                .is_err()
        );
        batch.checksum = format!("{:x}", Sha256::digest(encode(&batch.events).unwrap()));
        let mut bad = encode(&batch).unwrap();
        bad.push(b'\n');
        tokio::fs::write(&path, bad).await.unwrap();
        assert!(
            HostJournalReplay::open(&path, attempt, host, 0)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn flush_failure_never_advances_committed_sequence() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("journal");
        let attempt = Uuid::new_v4();
        let host = Uuid::new_v4();
        let mut journal = HostJournal::create(&path, attempt, host).await.unwrap();
        journal.append(event(1, attempt)).await.unwrap();
        journal.file = File::open(&path).await.unwrap(); // Read-only FD: deterministic write failure.
        assert!(journal.flush().await.is_err());
        assert_eq!(journal.committed_sequence(), 0);
        assert!(journal.read_after(0).await.is_err());
        assert!(journal.append(event(2, attempt)).await.is_err());
    }
}
