use chia_protocol::Bytes32;
use clap::{Parser, Subcommand};
use thiserror::Error;

#[derive(Debug, Parser)]
#[command(name = "streaming")]
#[command(about = "CLI used to interact with streamed CATs", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    #[command(arg_required_else_help = true)]
    Launch {
        asset_id: String,
        amount: String,
        start_timestamp: u64,
        end_timestamp: u64,
        recipient: String,
    },

    #[command(arg_required_else_help = true)]
    View { stream_id: String },

    #[command(arg_required_else_help = true)]
    Claim { stream_id: String },
}

#[derive(Error, Debug)]
enum CliError {
    #[error("Invalid asset id")]
    InvalidAssetId,
}

fn main() -> Result<(), CliError> {
    let args = Cli::parse();

    match args.command {
        Commands::Launch {
            asset_id,
            amount,
            start_timestamp,
            end_timestamp,
            recipient,
        } => {
            let asset_id = hex::decode(asset_id).map_err(|_| CliError::InvalidAssetId)?;

            // TODO: sage comms thingy
        }
        Commands::View { stream_id } => {
            println!("Viewing stream with stream_id={stream_id}");
        }
        Commands::Claim { stream_id } => {
            println!("Claiming stream with stream_id={stream_id}");
        }
    }

    Ok(())
}
