use anyhow::Result;
use async_std::channel::{bounded, Sender};
use chrono::prelude::*;
use clap::{Parser, Subcommand};
use serde_derive::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::File;
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
        limit: Option<usize>,
        offset: Option<usize>,
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
    Tags,
    Count,
    Save,
    Load,
    Select {
        #[clap(last = true)]
        value: Vec<String>,
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
        "{:<max_len$} #[{:<16}] @[{:<10}] ",
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
            } => value
                .iter()
                .map(|(idx, val)| format!("{}: {}", idx, format_item(val, true, *preview_length)))
                .collect::<Vec<String>>()
                .join("\n"),
            Payload::Value { value } => match value {
                Some(v) => v.to_owned(),
                _ => "".to_string(),
            },
            Payload::Message { value } => value.to_string(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Item {
    pub value: String,
    pub accessed_at: SystemTime,
    pub access_counter: u32,
    pub tags: Option<HashSet<String>>,
}

pub type Entries = std::collections::BTreeMap<u64, Item>;

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
            db: Some(String::from("./db.lisp")),
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
