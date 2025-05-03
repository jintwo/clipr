use anyhow::{Result, bail};
use clap::Parser;
use clipr_common::{Args, Config};
use std::sync::Arc;

use clipr_daemon::call;

fn main() -> Result<()> {
    let args = Args::parse();
    let config = Config::load_from_args(&args).map(Arc::new)?;

    if let Some(cmd) = args.command {
        match call(config, cmd) {
            Ok(payload) => println!("{}", String::from(&payload)),
            Err(err) => bail!(err),
        }
    }

    Ok(())
}
