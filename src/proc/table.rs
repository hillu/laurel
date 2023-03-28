use std::cmp::Ordering;
use std::collections::{BTreeMap, HashSet};
use std::error::Error;

use serde::{ser::SerializeMap, Serialize, Serializer};

use crate::label_matcher::LabelMatcher;
use crate::types::EventID;

#[derive(Clone, Debug, Default)]
pub struct ContainerInfo {
    pub id: Vec<u8>,
}

impl Serialize for ContainerInfo {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(Some(1))?;
        // safety: id contains entirely of hex-digits
        let converted = unsafe { std::str::from_utf8_unchecked(&self.id) };
        map.serialize_entry("id", converted)?;
        map.end()
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Process {
    pub key: ProcKey,
    pub ppid: Option<u32>,
    pub parent_key: Option<ProcKey>,
    pub labels: HashSet<Vec<u8>>,
    pub comm: Option<Vec<u8>>,
    pub exe: Option<Vec<u8>>,
    pub container_info: Option<ContainerInfo>,
}

impl Process {
    pub fn event_id(&self) -> Option<EventID> {
        match self.key {
            ProcKey::Event(id) => Some(id),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
pub enum ProcKey {
    Event(EventID),
    // FIXME: Decide: Should we mix in pid or some other data from /proc?
    Time(u64),
}

impl ProcKey {
    fn time(&self) -> u64 {
        match self {
            ProcKey::Event(id) => id.timestamp,
            ProcKey::Time(t) => *t,
        }
    }
}

impl Default for ProcKey {
    fn default() -> Self {
        ProcKey::Time(0)
    }
}

/// Ordering on ProcKey only takes the time component into
/// account.
impl Ord for ProcKey {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (ProcKey::Event(s), ProcKey::Event(o)) => s
                .timestamp
                .cmp(&o.timestamp)
                .then(s.sequence.cmp(&o.sequence)),
            (ProcKey::Time(s), ProcKey::Event(o)) => {
                s.partial_cmp(&o.timestamp).unwrap_or(Ordering::Less)
            }
            (ProcKey::Event(s), ProcKey::Time(o)) => {
                s.timestamp.partial_cmp(o).unwrap_or(Ordering::Greater)
            }
            (ProcKey::Time(s), ProcKey::Time(o)) => s.cmp(o),
        }
    }
}

impl PartialOrd for ProcKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Default, Serialize)]
pub struct ProcTable {
    procs: BTreeMap<ProcKey, Process>,
    // hints array to facilitate finding a process instance using its
    // PID. The list of process keys is to be kept sorted in
    // chronological order.
    by_pid: BTreeMap<u32, Vec<ProcKey>>,
}

impl ProcTable {
    /// Constructs process table from /proc entries
    // FIXME: Decide what to do with label config
    pub fn from_proc(
        label_exe: Option<&LabelMatcher>,
        propagate_labels: &HashSet<Vec<u8>>,
    ) -> Result<ProcTable, Box<dyn Error>> {
        unimplemented!()
    }

    /// Retrieves a process by pid.
    // FIXME: rename -> get_pid
    pub fn get_process(&mut self, pid: u32) -> Option<Process> {
        self.by_pid
            .get(&pid)
            .and_then(|keys| keys.last())
            .and_then(|key| self.procs.get(key))
            .cloned()
    }

    /// Retrieve a process by key.
    pub fn get(&self, key: &ProcKey) -> Option<Process> {
        self.procs.get(key).cloned()
    }

    /// Retrieve a process by key.
    ///
    /// Note: The `key' of the returned `Process' must not be changed!
    fn get_mut(&mut self, key: &ProcKey) -> Option<&mut Process> {
        self.procs.get_mut(key)
    }

    /// Retrieves a process by pid and latest start time
    pub fn get_pid_before(&self, pid: u32, time: u64) -> Option<Process> {
        self.by_pid
            .get(&pid)?
            .iter()
            .filter_map(|key| self.procs.get(key))
            .find(|proc| proc.key.time() < time)
            .cloned()
    }

    /// Adds a process.
    // TODO: rename, insert? (decide what to do with container info)
    pub fn add_process(
        &mut self,
        pid: u32,
        ppid: u32,
        id: EventID,
        comm: Option<Vec<u8>>,
        exe: Option<Vec<u8>>,
    ) {
        let key = ProcKey::Event(id);
        let parent_key = self
            .by_pid
            .get(&ppid)
            .and_then(|procs| procs.last())
            .cloned();
        let ppid = Some(ppid);
        let labels = HashSet::new();
        let container_info = None;
        self.procs.insert(
            key,
            Process {
                key,
                ppid,
                parent_key,
                comm,
                exe,
                labels,
                container_info,
            },
        );
        match self.by_pid.get_mut(&pid) {
            Some(ref mut v) => {
                v.push(key);
                v.sort();
            }
            None => {
                self.by_pid.insert(pid, vec![key]);
            }
        }
    }

    /// Add a label to a process
    pub fn add_label(&mut self, key: &ProcKey, label: &[u8]) {
        if let Some(ref mut proc) = self.get_mut(key) {
            proc.labels.insert(label.into());
        }
    }

    /// Remove a label from a process
    pub fn remove_label(&mut self, key: &ProcKey, label: &[u8]) {
        if let Some(ref mut proc) = self.get_mut(key) {
            proc.labels.remove(label);
        }
    }

    /// Add a label to a process
    // #[deprecated]
    pub fn add_label_pid(&mut self, pid: u32, label: &[u8]) {
        let key = match self.by_pid.get(&pid).and_then(|keys| keys.last()) {
            Some(&key) => key,
            None => return,
        };
        self.add_label(&key, label);
    }

    /// Remove a label from a process
    // #[deprecated]
    pub fn remove_label_pid(&mut self, pid: u32, label: &[u8]) {
        let key = match self.by_pid.get(&pid).and_then(|keys| keys.last()) {
            Some(&key) => key,
            None => return,
        };
        self.remove_label(&key, label)
    }

    /// Remove process entries that are no longer relevant.
    pub fn expire(&mut self) {
        unimplemented!()
    }
}
