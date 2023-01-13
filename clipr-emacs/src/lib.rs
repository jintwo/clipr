use anyhow::bail;
use chrono::prelude::*;
use clap::Parser;
use clipr_common::{shorten, Command, Config, Payload};
use emacs::IntoLisp;
use emacs::{Env, Result, Value};
use std::path::Path;
use std::sync::Arc;

// Emacs won't load the module without this.
emacs::plugin_is_GPL_compatible!();

static DEFAULT_CONFIG_PATH: &str = "~/config/clipr.toml";

// Register the initialization hook that Emacs will call when it loads the module.
#[emacs::module]
fn init(env: &Env) -> Result<Value<'_>> {
    env.message("Done loading!")
}

fn get_config_path(env: &Env) -> emacs::Result<emacs::Value<'_>> {
    let var = env.intern("clipr-config-path")?;
    let is_bound: bool = env.call("boundp", [var])?.is_not_nil();
    if !is_bound {
        return DEFAULT_CONFIG_PATH.to_string().into_lisp(env);
    }

    let config_path: String = env
        .call("symbol-value", [var])?
        .into_rust::<String>()
        .unwrap_or(DEFAULT_CONFIG_PATH.to_string());

    config_path.into_lisp(env)
}

fn payload_to_lisp<'a>(payload: &Payload, env: &'a Env) -> emacs::Result<emacs::Value<'a>> {
    match payload {
        Payload::Ok => "ok".to_string().into_lisp(env),
        Payload::Stop => "stop".to_string().into_lisp(env),
        Payload::List {
            value,
            preview_length,
        } => {
            let pos = env.intern(":pos")?;
            let content = env.intern(":content")?;
            let tags = env.intern(":tags")?;
            let date = env.intern(":date")?;

            let mut result: Vec<emacs::Value> = vec![];

            for (idx, item) in value.iter() {
                let item_tags = if let Some(tags) = &item.tags {
                    let mut ts = tags.iter().cloned().collect::<Vec<String>>();
                    ts.sort();
                    ts.join(":")
                } else {
                    "".to_string()
                };

                let item_date: String = DateTime::<Local>::from(item.accessed_at)
                    .format("%d-%m-%Y")
                    .to_string();

                let v = env.list((
                    pos,
                    *idx,
                    content,
                    shorten(&item.value, *preview_length),
                    tags,
                    item_tags,
                    date,
                    item_date,
                ))?;
                result.push(v);
            }

            Ok(env.list(result.as_slice())?)
        }
        Payload::Value { value } => match value {
            Some(v) => v.to_string().into_lisp(env),
            _ => "".to_string().into_lisp(env),
        },
        Payload::Message { value } => value.to_string().into_lisp(env),
    }
}

// Define a function callable by Lisp code.
#[emacs::defun]
fn cmd(env: &Env, value: String) -> emacs::Result<emacs::Value<'_>> {
    let config_path = get_config_path(env)?.into_rust::<String>()?;
    let config = Arc::new(Config::load_config(Path::new(&config_path))?);
    let mut cmd_line = shellwords::split(value.as_str()).unwrap();
    cmd_line.insert(0, "$bin_name".to_string());

    let cmd = match clipr_common::Args::try_parse_from(cmd_line) {
        Ok(args) => args.command.unwrap(),
        Err(_) => clipr_common::Command::Help,
    };

    match async_std::task::block_on(call(config, cmd)) {
        Ok(payload) => payload_to_lisp(&payload, env),
        Err(err) => bail!(err),
    }

    // TODO:
    // 4. use emacs table interface (see chuck plugin) [id, short-val, tags, date]
}

async fn call(config: Arc<Config>, cmd: Command) -> anyhow::Result<Payload, surf::Error> {
    let uri = format!("http://{}/command", config.listen_on());
    let req = surf::post(uri).body_json(&cmd)?;
    let rep: Payload = req.recv_json().await?;
    Ok(rep)
}
