use anyhow::{bail, Result};
use clap::Parser;
use clipr_common::{Args, Command, Config, Payload};
use std::{sync::Arc, time::Duration};

// TODO: move to common (http?)
fn call(config: Arc<Config>, cmd: Command) -> Result<Payload> {
    let uri = format!("http://{}/command", config.listen_on());
    let rep = reqwest::blocking::Client::new()
        .post(uri)
        .timeout(Duration::from_secs(2))
        .json(&cmd)
        .send()?
        .json::<Payload>()?;
    Ok(rep)
}

fn main() -> Result<()> {
    let args = Args::parse();
    let config = Arc::new(Config::load_from_args(&args)?);

    if let Some(cmd) = args.command {
        match call(config, cmd) {
            Ok(payload) => println!("{}", String::from(&payload)),
            Err(err) => bail!(err),
        }
    }

    Ok(())
}
