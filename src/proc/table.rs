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
    /// Unix timestamp with millisecond precision
    pub launch_time: u64,
    /// parent process id
    pub ppid: u32,
    pub labels: HashSet<Vec<u8>>,
    /// Event ID containing the event spawning this process entry
    /// (should be EXECVE).
    pub event_id: Option<EventID>,
    pub comm: Option<Vec<u8>>,
    pub exe: Option<Vec<u8>>,
    pub container_info: Option<ContainerInfo>,
}

#[derive(Debug, Serialize)]
pub enum ProcKey {
    Event(EventID),
    // FIXME: Decide: Should we mix in pid or some other data from /proc?
    Time(u64),
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
        unimplemented!()
    }

    pub fn get(&self, key: ProcKey) -> Option<Process> {
        unimplemented!()
    }

    pub fn get_pid_before(&self, pid: u32, time: u64) -> Option<Process> {
        unimplemented!()
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
        unimplemented!()
    }

    /// Add a label to a process
    // FIXME: Change to using ProcKey?
    pub fn add_label(&mut self, pid: u32, label: &[u8]) {
        unimplemented!()
    }

    /// Remove a label from a process
    // FIXME: Change to using ProcKey?
    pub fn remove_label(&mut self, pid: u32, label: &[u8]) {
        unimplemented!()
    }

    /// Remove process entries that are no longer relevant.
    pub fn expire(&mut self) {
        unimplemented!()
    }
}
