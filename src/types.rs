use std::collections::HashMap;
use std::convert::{TryFrom, TryInto};
use std::error::Error as StdError;
use std::fmt::{self, Debug, Display};
use std::iter::Iterator;
use std::ops::Range;
use std::str;
use std::string::*;

use lazy_static::lazy_static;

use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};

use crate::constants::*;
use crate::quoted_string::ToQuotedString;

/// The identifier of an audit event, corresponding to the
/// `msg=audit(…)` part of every _auditd(8)_ log line.
///
/// It consists of a mullisecond-precision timestamp and a sequence
/// number, thus guaranteeing per-host uniqueness.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy, Default)]
pub struct EventID {
    pub timestamp: u64,
    pub sequence: u32,
}

impl Display for EventID {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}.{:03}:{}",
            self.timestamp / 1000,
            self.timestamp % 1000,
            self.sequence
        )
    }
}

impl Serialize for EventID {
    #[inline(always)]
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(&format_args!(
            "{}.{:03}:{}",
            self.timestamp / 1000,
            self.timestamp % 1000,
            self.sequence
        ))
    }
}

/// The type of an audit message, corresponding to the `type=…` part
/// of every _auditd(8)_ log line.
///
/// The implementation uses the same 32bit unsigned integer that is
/// used by the Linux Audit API.
///
/// The mappings between numeric and symbolic values is generated
/// using CSV retrieved from the [`Linux Audit Project`]'s
/// documentation.
///
/// [`Linux Audit Project`]: https://github.com/linux-audit/audit-documentation
#[derive(PartialEq, Eq, Hash, Default, Clone, Copy)]
pub struct MessageType(pub u32);

impl Display for MessageType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match EVENT_NAMES.get(&(self.0)) {
            Some(name) => write!(f, "{}", name),
            None => write!(f, "UNKNOWN[{}]", self.0),
        }
    }
}

impl Debug for MessageType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "MessageType({})",
            match EVENT_NAMES.get(&(self.0)) {
                Some(name) => name.to_string(),
                None => format!("{}", self.0),
            }
        )
    }
}

impl Serialize for MessageType {
    #[inline(always)]
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match EVENT_NAMES.get(&(self.0)) {
            Some(name) => s.collect_str(name),
            None => s.collect_str(&format_args!("UNKNOWN[{}]", self.0)),
        }
    }
}

impl MessageType {
    /// True for messages that are part of multi-part events from
    /// kernel-space.
    ///
    /// This mimics auparse logic as of version 3.0.6
    pub fn is_multipart(&self) -> bool {
        (1300..1406).contains(&self.0)
            || (1420..2000).contains(&self.0)
            || (2001..2100).contains(&self.0)
            || self.0 == 1006
    }
}

/// Common values found in SYSCALL records
#[derive(Debug, PartialEq, Eq, Clone, Copy, Hash)]
pub enum Common {
    Arch,
    Syscall,
    Success,
    Exit,
    Items,
    PPid,
    Pid,
    Tty,
    Ses,
    Comm,
    Exe,
    Subj,
    Key,
}

const COMMON: &[(&str, Common)] = &[
    ("arch", Common::Arch),
    ("syscall", Common::Syscall),
    ("success", Common::Success),
    ("exit", Common::Exit),
    ("items", Common::Items),
    ("ppid", Common::PPid),
    ("pid", Common::Pid),
    ("tty", Common::Tty),
    ("ses", Common::Ses),
    ("comm", Common::Comm),
    ("exe", Common::Exe),
    ("subj", Common::Subj),
    ("key", Common::Key),
];

lazy_static! {
    static ref COMMON_TYPES: HashMap<&'static [u8], Common> = {
        let mut hm = HashMap::with_capacity(COMMON.len());
        for (name, value) in COMMON {
            hm.insert(name.as_bytes(), *value);
        }
        hm
    };
    static ref COMMON_NAMES: HashMap<Common, &'static str> = {
        let mut hm = HashMap::with_capacity(COMMON.len());
        for (name, value) in COMMON {
            hm.insert(*value, *name);
        }
        hm
    };
}

pub fn initialize() {
    lazy_static::initialize(&COMMON_TYPES);
    lazy_static::initialize(&COMMON_NAMES);
}

impl TryFrom<&[u8]> for Common {
    type Error = &'static str;
    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        COMMON_TYPES.get(&value).copied().ok_or("unknown key")
    }
}

impl From<Common> for &str {
    fn from(value: Common) -> Self {
        COMMON_NAMES[&value]
    }
}

impl Display for Common {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", COMMON_NAMES[self])
    }
}

pub(crate) type NVec = tinyvec::TinyVec<[u8; 14]>;

/// Representation of the key part of key/value pairs in [`Record`]
#[derive(PartialEq, Eq, Clone)]
pub enum Key {
    /// regular ASCII-only name as returned by parser
    Name(NVec),
    /// special case for *uid
    NameUID(NVec),
    /// special case for *gid
    NameGID(NVec),
    /// special case for common values
    Common(Common),
    /// regular ASCII-only name, output/serialization in all-caps, for
    /// translated / "enriched" values
    NameTranslated(NVec),
    /// `a0`, `a1`, `a2[0]`, `a2[1]`…
    Arg(u16, Option<u16>),
    /// `a0_len` …
    ArgLen(u16),
    Literal(&'static str),
}

impl Default for Key {
    fn default() -> Self {
        Key::Literal("no_key")
    }
}

impl Debug for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_string())
    }
}

impl Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Key::Arg(x, Some(y)) => write!(f, "a{}[{}]", x, y),
            Key::Arg(x, None) => write!(f, "a{}", x),
            Key::ArgLen(x) => write!(f, "a{}_len", x),
            Key::Name(r) | Key::NameUID(r) | Key::NameGID(r) => {
                // safety: The parser guarantees an ASCII-only key.
                let s = unsafe { str::from_utf8_unchecked(r) };
                f.write_str(s)
            }
            Key::Common(c) => write!(f, "{}", c),
            Key::NameTranslated(r) => {
                // safety: The parser guarantees an ASCII-only key.
                let s = unsafe { str::from_utf8_unchecked(r) };
                f.write_str(&str::to_ascii_uppercase(s))
            }
            Key::Literal(s) => f.write_str(s),
        }
    }
}

impl Serialize for Key {
    #[inline(always)]
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Key::Arg(x, Some(y)) => s.collect_str(&format_args!("a{}[{}]", x, y)),
            Key::Arg(x, None) => s.collect_str(&format_args!("a{}", x)),
            Key::ArgLen(x) => s.collect_str(&format_args!("a{}_len", x)),
            Key::Name(r) | Key::NameUID(r) | Key::NameGID(r) => {
                // safety: The parser guarantees an ASCII-only key.
                s.collect_str(unsafe { str::from_utf8_unchecked(r) })
            }
            Key::Common(c) => s.collect_str(&format_args!("{}", c)),
            Key::NameTranslated(r) => {
                // safety: The parser guarantees an ASCII-only key.
                s.collect_str(&str::to_ascii_uppercase(unsafe {
                    str::from_utf8_unchecked(r)
                }))
            }
            Key::Literal(l) => s.collect_str(l),
        }
    }
}

impl PartialEq<str> for Key {
    fn eq(&self, other: &str) -> bool {
        self == other.as_bytes()
    }
}

impl PartialEq<[u8]> for Key {
    fn eq(&self, other: &[u8]) -> bool {
        match self {
            Key::Name(r) | Key::NameUID(r) | Key::NameGID(r) => r.as_ref() == other,
            _ => self.to_string().as_bytes() == other,
        }
    }
}

/// Quotes in [`Value`] strings
#[derive(PartialEq, Eq, Clone, Copy)]
pub enum Quote {
    None,
    Single,
    Double,
    Braces,
}

#[derive(Clone, Copy)]
pub enum Number {
    Hex(u64),
    Dec(i64),
    Oct(u64),
}

impl Debug for Number {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Num:<{}>", self)
    }
}

impl Display for Number {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Number::Hex(n) => write!(f, "0x{:x}", n),
            Number::Dec(n) => write!(f, "{}", n),
            Number::Oct(n) => write!(f, "0o{:o}", n),
        }
    }
}

impl Serialize for Number {
    #[inline(always)]
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Number::Dec(n) => s.serialize_i64(*n),
            _ => s.collect_str(&format_args!("{}", self)),
        }
    }
}

/// Representation of the value part of key/value pairs in [`Record`]
#[derive(Clone)]
pub enum RecordValue {
    Empty,
    Str(Range<usize>, Quote),
    /// Segments are generated in Coalesce::normalize() from `EXECVE`
    /// / `aX[Y]` fragments.
    Segments(Vec<Range<usize>>),
    /// Lists are generated in Coalesce::normalize() e.g.: `EXECVE` /
    /// `a0`, `a1`, `a2` … -> `ARGV`
    List(Vec<RecordValue>),
    StringifiedList(Vec<RecordValue>),
    /// Key/Value map, used in ENV (environment variables) list
    Map(Vec<(SimpleRecordKey, SimpleRecordValue)>),
    /// Values generated in parse() from unquoted Str values
    ///
    /// For example, `SYSCALL` / `a0` etc are interpreted as
    /// hexadecimal numbers.
    Number(Number),
    /// Elements removed from ARGV lists
    Skipped((usize, usize)),
    Literal(&'static str),
}

#[derive(Clone)]
pub enum Value<'a> {
    Empty,
    Str(&'a [u8], Quote),
    Segments(Vec<&'a [u8]>),
    List(Vec<Value<'a>>),
    StringifiedList(Vec<Value<'a>>),
    Map(Vec<(SimpleKey<'a>, SimpleValue<'a>)>),
    Number(Number),
    Skipped((usize, usize)),
    Literal(&'static str),
}

impl<'a> Value<'a> {
    fn from_rv(r: &RecordValue, raw: &'a [u8]) -> Self {
        match r {
            RecordValue::Empty => Value::Empty,
            RecordValue::Str(r, q) => Value::Str(&raw[r.clone()], *q),
            RecordValue::Segments(v) => {
                Value::Segments(v.iter().map(|r| &raw[r.clone()]).collect())
            }
            RecordValue::List(v) => Value::List(v.iter().map(|v| Value::from_rv(v, raw)).collect()),
            RecordValue::StringifiedList(v) => {
                Value::StringifiedList(v.iter().map(|v| Value::from_rv(v, raw)).collect())
            }
            RecordValue::Map(v) => Value::Map(
                v.iter()
                    .map(|(k, v)| (SimpleKey::from_rv(k, raw), SimpleValue::from_rv(v, raw)))
                    .collect(),
            ),
            RecordValue::Number(n) => Value::Number(n.clone()),
            RecordValue::Skipped((a, b)) => Value::Skipped((*a, *b)),
            RecordValue::Literal(s) => Value::Literal(s),
        }
    }

    fn to_rv(&self, mut raw: &'a mut Vec<u8>) -> RecordValue {
        match self {
            Value::Empty => RecordValue::Empty,
            Value::Str(r, q) => {
                if let Some(offset) = slice_contains(raw, r) {
                    RecordValue::Str(offset..offset + r.len(), *q)
                } else {
                    let offset = raw.len();
                    raw.extend(*r);
                    RecordValue::Str(offset..offset + r.len(), *q)
                }
            }
            Value::Segments(v) => RecordValue::Segments(
                v.iter()
                    .map(|r| {
                        if let Some(offset) = slice_contains(raw, r) {
                            offset..offset + r.len()
                        } else {
                            let offset = raw.len();
                            raw.extend(*r);
                            offset..offset + r.len()
                        }
                    })
                    .collect(),
            ),
            Value::List(v) => RecordValue::List(v.iter().map(|el| el.to_rv(&mut raw)).collect()),
            Value::StringifiedList(v) => {
                RecordValue::StringifiedList(v.iter().map(|el| el.to_rv(&mut raw)).collect())
            }
            Value::Map(v) => RecordValue::Map(
                v.iter()
                    .map(|(k, v)| (k.to_rv(&mut raw), v.to_rv(&mut raw)))
                    .collect(),
            ),
            Value::Number(n) => RecordValue::Number(*n),
            Value::Skipped(n) => RecordValue::Skipped(*n),
            Value::Literal(s) => RecordValue::Literal(*s),
        }
    }
}

// FIXME: vocabulary
fn slice_contains(mainslice: &[u8], subslice: &[u8]) -> Option<usize> {
    let (m, s) = (mainslice.as_ptr_range(), subslice.as_ptr_range());
    if m.start <= s.start && m.end >= s.end {
        Some(unsafe { s.start.offset_from(m.start) } as _)
    } else {
        None
    }
}

impl RecordValue {
    pub fn str_len(&self) -> usize {
        match self {
            RecordValue::Str(r, _) => r.len(),
            RecordValue::Segments(vr) => vr.iter().map(|r| r.len()).sum(),
            _ => 0,
        }
    }
}

// FIXME: Str really needed? Perhaps Cow?
#[derive(Clone)]
pub enum SimpleRecordKey {
    Str(Range<usize>),
    Literal(&'static str),
}

#[derive(Clone)]
pub enum SimpleRecordValue {
    Str(Range<usize>),
    Number(Number),
}

// FIXME: just one type of str? Cow?
#[derive(Clone)]
pub enum SimpleKey<'a> {
    Str(&'a [u8]),
    Literal(&'static str),
}

impl<'a> SimpleKey<'a> {
    fn from_rv(r: &SimpleRecordKey, raw: &'a [u8]) -> Self {
        match r {
            SimpleRecordKey::Str(r) => SimpleKey::Str(&raw[r.clone()]),
            SimpleRecordKey::Literal(s) => SimpleKey::Literal(s),
        }
    }

    fn to_rv(&self, raw: &'a mut Vec<u8>) -> SimpleRecordKey {
        match self {
            SimpleKey::Str(s) => {
                if let Some(offset) = slice_contains(raw, s) {
                    SimpleRecordKey::Str(offset..offset + s.len())
                } else {
                    let offset = raw.len();
                    raw.extend(*s);
                    SimpleRecordKey::Str(offset..offset + s.len())
                }
            }
            SimpleKey::Literal(s) => SimpleRecordKey::Literal(s),
        }
    }
}

#[derive(Clone)]
pub enum SimpleValue<'a> {
    Str(&'a [u8]),
    Number(Number),
}

impl<'a> SimpleValue<'a> {
    fn from_rv(r: &SimpleRecordValue, raw: &'a [u8]) -> Self {
        match r {
            SimpleRecordValue::Str(r) => SimpleValue::Str(&raw[r.clone()]),
            SimpleRecordValue::Number(n) => SimpleValue::Number(n.clone()),
        }
    }

    fn to_rv(&self, raw: &'a mut Vec<u8>) -> SimpleRecordValue {
        match self {
            SimpleValue::Str(s) => {
                if let Some(offset) = slice_contains(raw, s) {
                    SimpleRecordValue::Str(offset..offset + s.len())
                } else {
                    let offset = raw.len();
                    raw.extend(*s);
                    SimpleRecordValue::Str(offset..offset + s.len())
                }
            }
            SimpleValue::Number(n) => SimpleRecordValue::Number(n.clone()),
        }
    }
}

/// List of [`Key`]/[`Value`] pairs, that are, for the most part,
/// stored offsets into the raw log line.
#[derive(Default, Clone)]
pub struct Record {
    // FIXME: make this an opaque type
    pub elems: Vec<(Key, RecordValue)>,
    pub raw: Vec<u8>,
}

impl Debug for Record {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut seq = f.debug_struct("Record");
        for (k, v) in self {
            seq.field(&*k.to_string(), &v);
        }
        seq.finish()
    }
}

impl Serialize for Record {
    #[inline(always)]
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(None)?;
        for (k, v) in self.into_iter() {
            match k {
                Key::Arg(_, _) | Key::ArgLen(_) => continue,
                _ => map.serialize_entry(&k, &v)?,
            }
        }
        map.end()
    }
}

impl Record {
    /// Merges two Records into one
    pub fn extend(&mut self, other: Self) {
        let rawlen = self.raw.len();
        self.raw.extend(other.raw);
        self.elems.extend(
            other
                .elems
                .into_iter()
                .map(|(k, v)| {
                    (
                        k,
                        match v {
                            RecordValue::Str(r, q) => RecordValue::Str(r.offset(rawlen), q),
                            RecordValue::Empty
                            | RecordValue::Number(_)
                            | RecordValue::Literal(_) => v,
                            RecordValue::Map(kv) => RecordValue::Map(
                                kv.into_iter()
                                    .map(|(k, v)| {
                                        (
                                            k,
                                            match v {
                                                SimpleRecordValue::Str(r) => {
                                                    SimpleRecordValue::Str(r.offset(rawlen))
                                                }
                                                _ => v,
                                            },
                                        )
                                    })
                                    .collect(),
                            ),
                            RecordValue::Segments(_) => {
                                panic!("RecordValue::Segments should only exist in EXECVE")
                            }
                            RecordValue::Skipped(_) => {
                                panic!("RecordValue::Skipped should only exist in EXECVE")
                            }
                            RecordValue::List(_) | RecordValue::StringifiedList(_) => {
                                panic!("RecordValue::List should only exist in EXECVE")
                            }
                        },
                    )
                })
                .collect::<Vec<_>>(),
        )
    }

    pub fn push(&mut self, kv: (Key, Value)) {
        let (k, v) = (kv.0, kv.1.to_rv(&mut self.raw));
        self.elems.push((k, v));
    }

    pub fn len(&self) -> usize {
        self.elems.len()
    }

    /// Retrieves the first value found for a given key
    pub fn get<K: AsRef<[u8]>>(&self, key: K) -> Option<Value> {
        let key = key.as_ref();
        for (k, v) in self {
            if format!("{}", k).as_bytes() == key {
                return Some(v);
            }
        }
        None
    }

    /// Add a byte string to a record.
    pub fn put<S: AsRef<[u8]>>(&mut self, s: S) -> Range<usize> {
        let s = s.as_ref();
        let b = self.raw.len();
        self.raw.extend(s);
        b..b + s.len()
    }
}

impl<'a> IntoIterator for &'a Record {
    type Item = (&'a Key, Value<'a>);
    type IntoIter = RecordIterator<'a>;
    fn into_iter(self) -> Self::IntoIter {
        RecordIterator { count: 0, r: self }
    }
}

pub struct RecordIterator<'a> {
    r: &'a Record,
    count: usize,
}

impl<'a> Iterator for RecordIterator<'a> {
    type Item = (&'a Key, Value<'a>);
    fn next(&mut self) -> Option<Self::Item> {
        self.count += 1;
        self.r
            .elems
            .get(self.count - 1)
            .map(|(key, value)| (key, Value::from_rv(value, &self.r.raw)))
    }
}

impl TryFrom<Value<'_>> for Vec<u8> {
    type Error = Box<dyn StdError>;
    fn try_from(v: Value) -> Result<Self, Self::Error> {
        match v {
            Value::Str(r, Quote::Braces) => {
                let mut s = Vec::with_capacity(r.len() + 2);
                s.push(b'{');
                s.extend(r);
                s.push(b'}');
                Ok(s)
            }
            Value::Str(r, _) => Ok(Vec::from(r)),
            Value::Empty => Ok("".into()),
            Value::Segments(ranges) => {
                let l = ranges.iter().map(|r| r.len()).sum();
                let mut sb = Vec::with_capacity(l);
                for r in ranges {
                    sb.extend(Vec::from(r));
                }
                Ok(sb)
            }
            Value::Number(_) => Err("Won't convert number to string".into()),
            Value::List(_) | Value::StringifiedList(_) => {
                Err("Can't convert list to scalar".into())
            }
            Value::Map(_) => Err("Can't convert map to scalar".into()),
            Value::Skipped(_) => Err("Can't convert skipped to scalar".into()),
            Value::Literal(s) => Ok(s.to_string().into()),
        }
    }
}

impl TryFrom<Value<'_>> for Vec<Vec<u8>> {
    type Error = Box<dyn StdError>;
    fn try_from(value: Value) -> Result<Self, Self::Error> {
        match value {
            Value::List(values) | Value::StringifiedList(values) => {
                let mut rv = Vec::with_capacity(values.len());
                for v in values {
                    let s = Vec::try_from(v)?;
                    rv.push(s);
                }
                Ok(rv)
            }
            _ => Err("not a list".into()),
        }
    }
}

impl Debug for Value<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Str(r, _q) => write!(f, "Str:<{}>", &String::from_utf8_lossy(&r)),
            Value::Empty => write!(f, "Empty"),
            Value::Segments(segs) => {
                write!(f, "Segments<")?;
                for (n, r) in segs.iter().enumerate() {
                    if n > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", String::from_utf8_lossy(&r))?;
                }
                write!(f, ">")
            }
            Value::List(vs) => {
                write!(f, "List:<")?;
                for (n, v) in vs.iter().enumerate() {
                    if n > 0 {
                        write!(f, ", ")?;
                    }
                    match v {
                        Value::Str(r, _) => {
                            write!(f, "{}", String::from_utf8_lossy(&r))?;
                        }
                        Value::Segments(rs) => {
                            for r in rs {
                                write!(f, "{}", String::from_utf8_lossy(&r))?;
                            }
                        }
                        Value::Number(n) => write!(f, "{:?}", n)?,
                        Value::Skipped(n) => {
                            write!(f, "Skip<elems{} bytes={}>", n.0, n.1)?;
                        }
                        Value::Empty => panic!("list can't contain empty value"),
                        Value::List(_) | Value::StringifiedList(_) => {
                            panic!("list can't contain list")
                        }
                        Value::Map(_) => panic!("list can't contain map"),
                        Value::Literal(v) => write!(f, "{:?}", v)?,
                    }
                }
                write!(f, ">")
            }
            Value::StringifiedList(vs) => {
                write!(f, "StringifiedList:<")?;
                for (n, v) in vs.iter().enumerate() {
                    if n > 0 {
                        write!(f, " ")?;
                    }
                    match v {
                        Value::Str(r, _) => {
                            write!(f, "{}", String::from_utf8_lossy(&r))?;
                        }
                        Value::Segments(rs) => {
                            for r in rs {
                                write!(f, "{}", String::from_utf8_lossy(&r))?;
                            }
                        }
                        Value::Number(n) => write!(f, "{:?}", n)?,
                        Value::Skipped(n) => {
                            write!(f, "Skip<elems={} bytes={}>", n.0, n.1)?;
                        }
                        Value::Empty => panic!("list can't contain empty value"),
                        Value::List(_) | Value::StringifiedList(_) => {
                            panic!("list can't contain list")
                        }
                        Value::Map(_) => panic!("List can't contain mapr"),
                        Value::Literal(v) => write!(f, "{}", v)?,
                    }
                }
                write!(f, ">")
            }
            Value::Map(vs) => {
                write!(f, "Map:<")?;
                for (n, v) in vs.iter().enumerate() {
                    if n > 0 {
                        write!(f, " ")?;
                    }
                    let v = match &v.1 {
                        SimpleValue::Str(r) => String::from_utf8_lossy(&r).into(),
                        SimpleValue::Number(n) => format!("{:?}", n),
                    };
                    write!(f, "{}={}", n, v)?;
                }
                write!(f, ">")
            }
            Value::Number(n) => write!(f, "{:?}", n),
            Value::Skipped(n) => write!(f, "Skip<elems={} bytes={}>", n.0, n.1),
            Value::Literal(s) => write!(f, "{:?}", s),
        }
    }
}

impl Serialize for Value<'_> {
    #[inline(always)]
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Value::Empty => s.serialize_none(),
            Value::Str(r, q) => {
                let (q1, q2) = if let Quote::Braces = q {
                    ("{", "}")
                } else {
                    ("", "")
                };
                s.collect_str(&format_args!("{}{}{}", q1, &r.to_quoted_string(), q2))
            }
            Value::Segments(segs) => {
                let l = segs.iter().map(|r| r.len()).sum();
                let mut sb = String::with_capacity(l);
                for seg in segs {
                    sb.push_str(&seg.to_quoted_string());
                }
                s.collect_str(&sb)
            }
            Value::List(vs) => s.collect_seq(vs),
            Value::StringifiedList(vs) => {
                let mut buf: Vec<u8> = Vec::with_capacity(vs.len());
                let mut first = true;
                for v in vs {
                    if first {
                        first = false;
                    } else {
                        buf.push(b' ');
                    }
                    if let Value::Skipped((args, bytes)) = v {
                        buf.extend(
                            format!("<<< Skipped: args={}, bytes={} >>>", args, bytes).bytes(),
                        );
                    } else {
                        buf.extend((*v).clone().try_into().unwrap_or_else(|_| vec![b'x']))
                    }
                }
                s.serialize_str(&buf.to_quoted_string())
            }
            Value::Number(n) => n.serialize(s),
            Value::Map(vs) => {
                let mut map = s.serialize_map(Some(vs.len()))?;
                for (k, v) in vs {
                    match k {
                        SimpleKey::Str(r) => map.serialize_key(&r.to_quoted_string())?,
                        SimpleKey::Literal(n) => map.serialize_key(n)?,
                    }
                    match v {
                        SimpleValue::Str(r) => map.serialize_value(&r.to_quoted_string())?,
                        SimpleValue::Number(n) => map.serialize_value(&n)?,
                    }
                }
                map.end()
            }
            Value::Skipped((args, bytes)) => {
                let mut map = s.serialize_map(Some(2))?;
                map.serialize_entry("skipped_args", args)?;
                map.serialize_entry("skipped_bytes", bytes)?;
                map.end()
            }
            Value::Literal(v) => s.collect_str(v),
        }
    }
}

impl PartialEq<str> for Value<'_> {
    fn eq(&self, other: &str) -> bool {
        self == other.as_bytes()
    }
}

impl PartialEq<[u8]> for Value<'_> {
    fn eq(&self, other: &[u8]) -> bool {
        if let Ok(v) = self.clone().try_into() as Result<Vec<u8>, _> {
            return v == other;
        }
        false
    }
}

/// The Offset trait provides an implementation for adding offset to Range.
trait Offset {
    fn offset(&self, offset: usize) -> Self;
}

impl Offset for Range<usize> {
    fn offset(&self, offset: usize) -> Self {
        Range {
            start: self.start + offset,
            end: self.end + offset,
        }
    }
}
