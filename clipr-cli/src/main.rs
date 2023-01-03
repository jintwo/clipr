use anyhow::{bail, Result};
use clap::Parser;
use clipr_common::{Args, Command, Config, Payload};
use std::sync::Arc;

async fn call(config: Arc<Config>, cmd: Command) -> Result<Payload, surf::Error> {
    let uri = format!(
        "http://{}:{}/command",
        &config.host.as_ref().unwrap(),
        &config.json_port.unwrap()
    );
    let req = surf::post(uri).body_json(&cmd)?;
    let rep: Payload = req.recv_json().await?;
    Ok(rep)
}

#[async_std::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let config = Arc::new(Config::load_from_args(&args)?);

    if let Some(cmd) = args.command {
        match call(config, cmd).await {
            Ok(payload) => println!("{}", String::from(&payload)),
            Err(err) => bail!(err),
        }
    }

    Ok(())
}
