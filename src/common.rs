use std::collections::VecDeque;
use std::str::FromStr;
use thiserror::Error;

pub const HEADER_LEN: usize = 8;

#[derive(Error, Debug)]
pub enum CommandParseError {
    #[error("empty command")]
    EmptyCommand,
    #[error("invalid command `{0}`")]
    InvalidCommand(String),
    #[error("insufficient arguments")]
    InsufficientArgs,
    #[error("invalid argument type `{0}`")]
    InvalidArgType(String),
}

#[derive(Debug)]
pub enum Command {
    Add(String),
    Del(u32),
    List,
    Show(u32),
    Set(u32),
    Load(String),
    Tag(u32, String),
    Quit,
    Invalid(CommandParseError),
}

impl FromStr for Command {
    type Err = CommandParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts: VecDeque<&str> = s.split(' ').collect();

        let cmd = parts.pop_front().unwrap();
        if cmd.is_empty() {
            return Err(CommandParseError::EmptyCommand);
        }

        match cmd {
            "list" => Ok(Command::List),
            "quit" => Ok(Command::Quit),
            "add" | "show" | "set" | "load" | "del" if parts.is_empty() => {
                Err(CommandParseError::InsufficientArgs)
            }
            "add" => {
                let args = parts.make_contiguous().join(" ");
                Ok(Command::Add(args))
            }
            "show" => {
                if let Ok(arg) = parts[0].parse() {
                    Ok(Command::Show(arg))
                } else {
                    Err(CommandParseError::InvalidArgType(parts[0].to_owned()))
                }
            }
            "set" => {
                if let Ok(arg) = parts[0].parse() {
                    Ok(Command::Set(arg))
                } else {
                    Err(CommandParseError::InvalidArgType(parts[0].to_owned()))
                }
            }
            "del" => {
                if let Ok(arg) = parts[0].parse() {
                    Ok(Command::Del(arg))
                } else {
                    Err(CommandParseError::InvalidArgType(parts[0].to_owned()))
                }
            }
            "load" => {
                let filename = parts[0].to_owned();
                Ok(Command::Load(filename))
            }
            "tag" if parts.len() < 2 => Err(CommandParseError::InsufficientArgs),
            "tag" => {
                if let Ok(idx) = parts[0].parse() {
                    let tag = parts[1].to_owned();
                    Ok(Command::Tag(idx, tag))
                } else {
                    Err(CommandParseError::InvalidArgType(parts[0].to_owned()))
                }
            }
            _ => Err(CommandParseError::InvalidCommand(cmd.to_owned())),
        }
    }
}

impl From<Command> for Vec<u8> {
    fn from(cmd: Command) -> Self {
        let s = match cmd {
            Command::List | Command::Invalid(_) => "list".to_string(),
            Command::Quit => "quit".to_string(),
            Command::Add(v) => format!("add {}", v),
            Command::Del(i) => format!("del {}", i),
            Command::Set(i) => format!("set {}", i),
            Command::Tag(i, v) => format!("tag {} {}", i, v),
            Command::Show(i) => format!("show {}", i),
            Command::Load(v) => format!("load {}", v),
        };

        s.as_bytes().to_vec()
    }
}

#[derive(Debug)]
pub enum Response {
    Data(String),
    NewItem(String),
    Ok,
    Stop,
}
