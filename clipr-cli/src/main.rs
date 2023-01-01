use anyhow::{bail, Result};
use async_std::task;
use clap::Parser;
use clipr_common::{Args, Command, Config, Payload};
use std::sync::Arc;

async fn call(config: Arc<Config>, cmd: Command) -> Result<Payload, surf::Error> {
    let connect_to = format!(
        "{}:{}",
        &config.host.as_ref().unwrap(),
        &config.json_port.unwrap()
    );
    let uri = format!("http://{}/command", connect_to);
    let req = surf::post(uri).body_json(&cmd)?;
    let rep: Payload = req.recv_json().await?;
    Ok(rep)
}

fn main() -> Result<()> {
    let args = Args::parse();
    let config = Arc::new(Config::load_from_args(&args)?);

    match args.command {
        Some(cmd) => match task::block_on(call(config, cmd)) {
            Ok(payload) => {
                println!("{}", String::from(&payload));
                Ok(())
            }
            Err(err) => bail!(err),
        },
        None => Ok(()),
    }
}
