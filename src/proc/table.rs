use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::error::Error;

use serde::{ser::SerializeMap, Serialize, Serializer};

use super::parser::{get_pids, parse_proc_pid};

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
    label_exe: Option<LabelMatcher>,
    propagate_labels: HashSet<Vec<u8>>,
}

impl ProcTable {
    pub fn set_label_exe(mut self, label_exe: Option<LabelMatcher>) -> Self {
        self.label_exe = label_exe;
        self
    }
    pub fn set_propagate_labels(mut self, propagate_labels: HashSet<Vec<u8>>) -> Self {
        self.propagate_labels = propagate_labels;
        self
    }
    pub fn init_from_proc(mut self) -> Result<Self, Box<dyn Error>> {
        for pid in get_pids()? {
            let pi = parse_proc_pid(pid)?;
            let key = ProcKey::Time(pi.starttime);
            let labels = HashSet::new();
            let (comm, exe) = (pi.comm, pi.exe);
            let container_info = pi.container_id.map(|ci| ContainerInfo{id: ci});
            // FIXME: We can't use ppid until we figure out how to
            // detect if a process might have been reparented after
            // its parent has exited. It may have been become a child
            // of a process != pid1 if PR_SET_CHILD_SUBREAPER has been
            // used.
            //
            // Idea: Is the process pid1 in its own namespace?
            //
            // Idea: Exceptions by basename(exe): aSearching for
            // prctl.*PR_SET_CHILD_SUBREAPER in codesearch.debian.net:
            // 
            // - systemd
            // - lutris-wrapper
            // - tini
            // - bubblewrap
            // - runc
            // - conmon
            // - crun
            // - keepalived
            // - lxqt-session
            // - catatonit
            // - criu
            self.procs.insert(
                key,
                Process {
                    key,
                    ppid: None,
                    parent_key: None,
                    comm,
                    exe,
                    labels,
                    container_info,
                },
            );
            self.by_pid.insert(pid, vec![key]);
        }

        // initialize labels
        if let Some(ref label_exe) = self.label_exe {
            for proc in self.procs.values_mut() {
                if let Some(ref exe) = proc.exe {
                    proc.labels
                        .extend(label_exe.matches(exe).iter().map(|v| Vec::from(*v)));
                }
            }
        }

        Ok(self)
    }

    /// Constructs process table from /proc entries
    // #[deprecated]
    pub fn from_proc(
        label_exe: Option<&LabelMatcher>,
        propagate_labels: &HashSet<Vec<u8>>,
    ) -> Result<ProcTable, Box<dyn Error>> {
        ProcTable::default()
            .set_label_exe(label_exe.cloned())
            .set_propagate_labels(propagate_labels.clone())
            .init_from_proc()
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
        let mut proc_prune: BTreeSet<ProcKey> = self.procs.keys().cloned().collect();
        let mut pid_prune: Vec<u32> = vec![];

        let live_processes = match get_pids() {
            Ok(p) => p,
            Err(_) => return,
        };
        // unmark latest instance in by_pids and all its parents
        for seed_pid in live_processes {
            let mut key = match self.by_pid.get(&seed_pid).and_then(|keys| keys.last()) {
                None => continue,
                Some(&key) => key,
            };

            loop {
                if proc_prune.remove(&key) {
                    break;
                }
                match self.procs.get(&key).and_then(|proc| proc.parent_key) {
                    Some(parent_key) => key = parent_key,
                    None => break,
                };
            }
        }
        // remove entries from primary process list
        for key in proc_prune.iter() {
            self.procs.remove(key);
        }
        // rewrite by_pid hints
        for (pid, procs) in self.by_pid.iter_mut() {
            *procs = procs
                .iter()
                .filter(|proc| !proc_prune.contains(proc))
                .cloned()
                .collect();
            if procs.is_empty() {
                pid_prune.push(*pid);
            }
        }
        for pid in pid_prune {
            self.by_pid.remove(&pid);
        }
    }
}
