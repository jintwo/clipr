use anyhow::Result;
use async_std::channel::{bounded, Sender};
use chrono::prelude::*;
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashSet, LinkedList};
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::io::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

pub const HEADER_LEN: usize = 8;

pub enum Request {
    Sync(String),
    Command(Command, Sender<Response>),
    Quit,
}

impl Request {
    pub async fn send_cmd(sender: &Sender<Request>, cmd: Command) -> Option<Response> {
        let (tx, rx) = bounded::<Response>(1);
        sender.send(Request::Command(cmd, tx)).await.unwrap();
        rx.recv().await.ok()
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Response {
    NewItem(String),
    Payload(Payload),
    Ok,
    Stop,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Payload {
    Ok,
    List {
        value: Vec<(usize, Item)>,
        preview_length: Option<usize>,
    },
    Value {
        value: Option<String>,
    },
    Message {
        // TODO: drop me?
        value: String,
    },
    Stop,
}

#[derive(Debug, Subcommand, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Command {
    Add {
        #[clap(last = true)]
        value: Vec<String>,
    },
    Del {
        from_index: usize,
        to_index: Option<usize>,
    },
    List {
        from_index: Option<usize>,
        to_index: Option<usize>,
        preview_length: Option<usize>,
    },
    Get {
        index: usize,
    },
    Set {
        index: usize,
    },
    Insert {
        filename: String,
    },
    Tag {
        index: usize,
        tag: String,
    },
    Untag {
        index: usize,
        tag: String,
    },
    Pin {
        index: usize,
        pin: char,
    },
    Unpin {
        index: usize,
    },
    Tags,
    Count,
    Save,
    Load,
    Select {
        #[clap(long)]
        set: bool,

        // #[clap(last = true)]
        // value: Vec<String>,
        #[clap(long)]
        pin: Option<String>,

        #[clap(long, action = clap::ArgAction::Append)]
        tag: Vec<String>,

        #[clap(long)]
        value: Option<String>,
    },
    Help,
    Quit,
}

pub fn format_item(item: &Item, short: bool, preview_length: Option<usize>) -> String {
    let val = if short {
        shorten(&item.value, preview_length)
    } else {
        item.value.clone()
    };

    let tags = match &item.tags {
        Some(tags) => {
            let mut ts = tags.iter().map(|v| v.as_str()).collect::<Vec<&str>>();
            ts.sort();
            ts.join(",")
        }
        None => "".to_string(),
    };

    let dt: DateTime<Local> = item.accessed_at.into();
    let max_len = preview_length.unwrap_or(MAX_LEN);

    format!(
        "[{:1}] {:<max_len$} #[{:<16}] @[{:<10}] ",
        item.pin.unwrap_or(' '),
        val,
        tags,
        dt.format("%d-%m-%Y")
    )
}

fn _has_newlines(s: &str) -> Option<usize> {
    s.as_bytes()
        .iter()
        .enumerate()
        .find(|&(_, c)| *c == b'\n')
        .map(|(i, _)| i)
}

pub const MAX_LEN: usize = 64;
const SPACER_LEN: usize = 4;
const PREFIX_LEN: usize = 16;

pub fn shorten(s: &str, max_len: Option<usize>) -> String {
    let chars = s.chars();
    let length = s.chars().count();
    let max_len = max_len.unwrap_or(MAX_LEN);

    // TODO:
    // 0. if has length > MAX_LEN -> S[0...PREFIX_LEN]...S[-PREFIX_LEN...]
    // 1. if has whitespaces until prefix-len -> S[0...PREFIX_LEN]...
    // 2. if has whitespaces after spacer -> S[0...PREFIX_LEN]...

    let mut short = if length > max_len {
        chars.enumerate().fold(String::new(), |acc, (i, c)| {
            if i < PREFIX_LEN || i > length - PREFIX_LEN {
                format!("{acc}{c}")
            } else if i > PREFIX_LEN && i < (PREFIX_LEN + SPACER_LEN) {
                format!("{acc}.")
            } else {
                acc
            }
        })
    } else {
        chars.collect::<String>()
    };

    let newline_offset = _has_newlines(short.as_str()).unwrap_or(short.len());
    let rest = short.split_off(newline_offset);
    if rest.chars().any(|c| !c.is_whitespace()) {
        format!("{short}...")
    } else {
        short.to_string()
    }
}

impl From<&Payload> for String {
    fn from(payload: &Payload) -> Self {
        match payload {
            Payload::Ok => "ok".to_string(),
            Payload::Stop => "stop".to_string(),
            Payload::List {
                value,
                preview_length,
            } => {
                let places = value.len().to_string().len();
                value
                    .iter()
                    .map(|(index, val)| {
                        format!(
                            "{:>places$}: {}",
                            index,
                            format_item(val, true, *preview_length)
                        )
                    })
                    .collect::<Vec<String>>()
                    .join("\n")
            }
            Payload::Value { value } => match value {
                Some(v) => v.to_owned(),
                _ => "".to_string(),
            },
            Payload::Message { value } => value.to_string(),
        }
    }
}

pub fn calculate_hash<T: Hash>(v: &T) -> u64 {
    let mut h = DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Item {
    pub value: String,
    pub access_counter: u32,
    pub accessed_at: SystemTime,
    pub tags: Option<HashSet<String>>,
    pub pin: Option<char>,
}

impl Item {
    pub fn new(value: String) -> Self {
        Self {
            value,
            access_counter: 1,
            accessed_at: SystemTime::now(),
            tags: None,
            pin: None,
        }
    }

    pub fn touch(&mut self) {
        self.accessed_at = SystemTime::now();
        self.access_counter += 1;
    }
}

impl From<String> for Item {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Entries {
    pub values: LinkedList<Item>,
    pub hashes: LinkedList<u64>,
}

impl Default for Entries {
    fn default() -> Self {
        Self::new()
    }
}

fn _drop_list_values<T>(from_index: usize, to_index: Option<usize>, list: &mut LinkedList<T>) {
    let to_index = to_index.unwrap_or(from_index + 1);
    let mut upper = list.split_off(from_index);
    let mut rest = upper.split_off(to_index - from_index);
    list.append(&mut rest);
}

fn _find_list_element<T>(value: &T, list: &LinkedList<T>) -> Option<usize>
where
    T: PartialEq<T>,
{
    list.iter()
        .enumerate()
        .find(|&(_, h)| h == value)
        .map(|(index, _)| index)
}

impl Entries {
    pub fn new() -> Self {
        Entries {
            values: LinkedList::new(),
            hashes: LinkedList::new(),
        }
    }

    // INFO: values + hashes should be consistent. in the name of DOD ;)
    pub fn insert(&mut self, value: String) {
        let hash = calculate_hash(&value);

        if let Some(index) = _find_list_element(&hash, &self.hashes) {
            let mut values_tail = self.values.split_off(index);
            if let Some(mut elt) = values_tail.pop_front() {
                elt.touch();
                self.values.push_front(elt);
                self.values.append(&mut values_tail);
            }

            let mut hashes_tail = self.hashes.split_off(index);
            if let Some(elt) = hashes_tail.pop_front() {
                self.hashes.push_front(elt);
                self.hashes.append(&mut hashes_tail);
            }
        } else {
            self.hashes.push_front(hash);
            self.values.push_front(value.into());
        }
    }

    pub fn delete(&mut self, from_index: usize, to_index: Option<usize>) {
        _drop_list_values(from_index, to_index, &mut self.values);
        _drop_list_values(from_index, to_index, &mut self.hashes);
    }

    pub fn get(&mut self, index: usize) -> Option<&mut Item> {
        self.values
            .iter_mut()
            .enumerate()
            .find(|(i, _)| index == *i)
            .map(|(_, item)| item)
    }

    pub fn get_value(&mut self, index: usize) -> Option<String> {
        self.get(index).map(|item| item.value.clone())
    }

    pub fn select_by_range(
        &self,
        from_index: Option<usize>,
        to_index: Option<usize>,
    ) -> Vec<(usize, Item)> {
        let from_index = from_index.unwrap_or(0);
        let to_index = to_index.unwrap_or(self.values.len());

        self.values
            .iter()
            .enumerate()
            .filter(|(index, _item)| *index >= from_index && *index < to_index)
            .map(|(index, item)| (index, item.clone()))
            .collect()
    }

    pub fn select(
        &self,
        pin: Option<char>,
        tag: Vec<String>,
        value: Option<String>,
    ) -> Vec<(usize, Item)> {
        // return ALL or NONE?
        if pin.is_none() && tag.is_empty() && value.is_none() {
            return vec![];
        };

        if let Some(c) = pin {
            return self.select_by_pin(c);
        }

        let mut items_iter: Box<dyn Iterator<Item = (usize, &Item)>> =
            Box::new(self.values.iter().enumerate());

        if !tag.is_empty() {
            let tags_set: HashSet<String> = tag.into_iter().collect();
            items_iter = Box::new(items_iter.filter(move |(_, item)| {
                if let Some(tags) = &item.tags {
                    tags.is_superset(&tags_set)
                } else {
                    false
                }
            }));
        }

        if let Some(value) = value {
            items_iter = Box::new(items_iter.filter(move |(_, item)| item.value.contains(&value)));
        }

        items_iter
            .map(|(index, item)| (index, item.clone()))
            .collect()
    }

    pub fn select_by_value(&self, value: String) -> Vec<(usize, Item)> {
        let val = value.as_str();

        self.values
            .iter()
            .enumerate()
            .filter(|(_, item)| item.value.contains(val))
            .map(|(index, item)| (index, item.clone()))
            .collect()
    }

    pub fn select_by_tag(&self, tag: String) -> Vec<(usize, Item)> {
        self.values
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                if let Some(tags) = &item.tags {
                    tags.get(&tag).is_some()
                } else {
                    false
                }
            })
            .map(|(index, item)| (index, item.clone()))
            .collect()
    }

    pub fn select_by_pin(&self, pin: char) -> Vec<(usize, Item)> {
        if let Some((index, item)) = self
            .values
            .iter()
            .enumerate()
            .find(|(_, item)| item.pin.unwrap_or(' ') == pin)
        {
            vec![(index, item.clone())]
        } else {
            vec![]
        }
    }
    pub fn tag(&mut self, index: usize, tag: String) -> bool {
        if let Some(item) = self.get(index) {
            item.tags
                .get_or_insert(HashSet::<String>::new())
                .insert(tag);
            true
        } else {
            false
        }
    }

    pub fn untag(&mut self, index: usize, tag: String) -> bool {
        if let Some(item) = self.get(index) {
            match item.tags.as_mut() {
                Some(ts) => ts.remove(&tag),
                _ => true,
            }
        } else {
            false
        }
    }

    pub fn get_tags(&self) -> HashSet<String> {
        let mut result: HashSet<String> = HashSet::new();
        for item in self.values.iter() {
            if let Some(tags) = item.tags.as_ref() {
                result = result.union(tags).cloned().collect();
            }
        }
        result
    }

    pub fn pin(&mut self, index: usize, pin: char) {
        self.values.iter_mut().for_each(|item| match item.pin {
            Some(p) if p == pin => {
                item.pin.take();
            }
            _ => {}
        });

        if let Some(item) = self.get(index) {
            item.pin.replace(pin);
        }
    }

    pub fn unpin(&mut self, index: usize) {
        if let Some(item) = self.get(index) {
            item.pin.take();
        }
    }

    pub fn len(&self) -> usize {
        let values_len = self.values.len();
        let hashes_len = self.hashes.len();
        if values_len != hashes_len {
            eprintln!(
                "Inconsistent state ({} values against {} hashes). Need to rebuild index.",
                hashes_len, values_len
            )
        }
        values_len
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[derive(Debug, Deserialize)]
pub struct Config {
    pub interactive: Option<bool>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub db: Option<String>,
}

impl Config {
    pub fn listen_on(&self) -> String {
        format!("{}:{}", self.host.as_ref().unwrap(), self.port.unwrap())
    }
}

pub struct State {
    pub config: Config,
    pub entries: Mutex<Entries>,
}

impl State {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            entries: Mutex::new(Entries::new()),
        }
    }
}

#[derive(Parser, Debug)]
pub struct Args {
    #[clap(short, long, value_parser)]
    pub config: Option<PathBuf>,
    #[clap(subcommand)]
    pub command: Option<Command>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            host: Some(String::from("127.0.0.1")),
            port: Some(8932),
            interactive: Some(true),
            db: Some(String::from("./db.json")),
        }
    }
}

impl Config {
    pub fn load_config(filename: &Path) -> Result<Config> {
        let mut file = File::open(filename)?;
        let mut buffer = String::new();
        file.read_to_string(&mut buffer)?;

        let config: Config = toml::from_str(buffer.as_str())?;

        Ok(config)
    }

    pub fn load_from_args(args: &Args) -> Result<Self> {
        Ok(if let Some(filename) = args.config.as_deref() {
            Self::load_config(filename)?
        } else {
            Self::default()
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_entries_insert() {
        let mut entries = Entries::default();
        entries.insert(String::from("hello"));
        assert_eq!(entries.values.len(), 1);
        assert_eq!(entries.hashes.len(), 1);
    }

    #[test]
    fn test_entries_get() {
        let value = String::from("hello");
        let mut entries = Entries::default();
        entries.insert(value.clone());
        assert_eq!(entries.get(0).unwrap().value, value);
    }
}
