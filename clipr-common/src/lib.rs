use anyhow::Result;
use async_std::channel::Sender;
use async_std::net::TcpStream;
use async_std::prelude::*;
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

#[derive(Debug)]
pub enum Response {
    NewItem(String),
    Payload(Payload),
    Ok,
    Stop,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Payload {
    Ok,
    List { value: Vec<(usize, String)> },
    Value { value: Option<String> },
    Message { value: String },
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

fn command_to_vec(cmd: &Command) -> Vec<u8> {
    let s = match cmd {
        Command::List { limit, offset } => match (limit, offset) {
            (Some(limit), offset) => format!("list {} {}", limit, offset.unwrap_or(0)),
            (None, _) => "list".to_string(),
        },
        Command::Count => "count".to_string(),
        Command::Save => "save".to_string(),
        Command::Load => "load".to_string(),
        Command::Add { value } => format!("add -- {}", value.join(" ")),
        Command::Del {
            from_index,
            to_index,
        } => format!("del {} {}", from_index, to_index.unwrap_or(*from_index)),
        Command::Set { index } => format!("set {}", index),
        Command::Tag { index, tag } => format!("tag {} {}", index, tag),
        Command::Get { index } => format!("get {}", index),
        Command::Insert { filename } => format!("insert {}", filename),
        Command::Select { value } => format!("select -- {}", value.join(" ")),
        Command::Help => "help".to_string(),
        Command::Quit => "quit".to_string(),
    };

    s.as_bytes().to_vec()
}

impl From<Command> for Vec<u8> {
    fn from(cmd: Command) -> Self {
        command_to_vec(&cmd)
    }
}

impl From<&Command> for Vec<u8> {
    fn from(cmd: &Command) -> Self {
        command_to_vec(cmd)
    }
}

impl From<&Payload> for String {
    fn from(payload: &Payload) -> Self {
        match payload {
            Payload::Ok => "ok".to_string(),
            Payload::Stop => "stop".to_string(),
            Payload::List { value } => value
                .iter()
                .map(|(idx, val)| format!("{}: {}", idx, val))
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

pub async fn read_raw_command(stream: &TcpStream) -> Result<Command> {
    let mut reader = stream.clone();

    // read header
    let mut header: [u8; HEADER_LEN] = [0; HEADER_LEN];
    reader.read_exact(&mut header).await?;
    let buf_len = usize::from_le_bytes(header);

    // read payload
    let mut buf = vec![0u8; buf_len];
    reader.read_exact(&mut buf).await?;
    let payload = String::from_utf8_lossy(&buf[..]);

    // parse command
    let mut cmd_line = shellwords::split(&payload).unwrap();
    let bin_name = std::env::args().next().unwrap();
    cmd_line.insert(0, bin_name);

    let args = Args::try_parse_from(cmd_line)?;
    Ok(args.command.unwrap())
}

pub async fn read_json_command(stream: &TcpStream) -> Result<Command> {
    let mut reader = stream.clone();

    // read header
    let mut header: [u8; HEADER_LEN] = [0; HEADER_LEN];
    reader.read_exact(&mut header).await?;
    let buf_len = usize::from_le_bytes(header);

    // read payload
    let mut buf = vec![0u8; buf_len];
    reader.read_exact(&mut buf).await?;
    let payload = String::from_utf8_lossy(&buf[..]);

    // parse command
    let mut cmd_line = shellwords::split(&payload).unwrap();
    let bin_name = std::env::args().next().unwrap();
    cmd_line.insert(0, bin_name);

    let args = Args::try_parse_from(cmd_line)?;
    Ok(args.command.unwrap())
}

pub async fn write_command(stream: &mut TcpStream, cmd: Command) -> Result<()> {
    // encode payload
    let payload: Vec<u8> = cmd.into();

    // write header
    let buf_len = payload.len();
    let header = &buf_len.to_le_bytes()[..HEADER_LEN];
    stream.write_all(header).await?;

    // write payload
    stream.write_all(&payload[..]).await?;
    stream.flush().await?;

    Ok(())
}

#[derive(serde_derive::Serialize, serde_derive::Deserialize)]
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
    pub raw_port: Option<u16>,
    pub json_port: Option<u16>,
    pub db: Option<String>,
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
            raw_port: Some(8931),
            json_port: Some(8932),
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
