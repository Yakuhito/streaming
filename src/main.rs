use chia::puzzles::cat::CatArgs;
use chia_protocol::{Bytes32, Coin};
use chia_wallet_sdk::{decode_address, encode_address, ClientError};
use clap::{Parser, Subcommand};
use clvm_utils::CurriedProgram;
use dirs::home_dir;
use std::path::{Path, PathBuf};
use streaming::{streamed_cat, StreamPuzzle2ndCurryArgs, StreamedCat};
use thiserror::Error;

mod client;

use client::{Amount, SageClient, SendCat};

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
        #[arg(long, default_value = "~/.local/share/com.rigidnetwork.sage/ssl")]
        cert_path: String,
        #[arg(long, default_value = "0.0001")]
        fee: String,
        #[arg(long, default_value_t = false)]
        mainnet: bool,
    },

    #[command(arg_required_else_help = true)]
    View {
        stream_id: String,
        #[arg(long, default_value = "~/.local/share/com.rigidnetwork.sage/ssl")]
        cert_path: String,
        #[arg(long, default_value = "0.0001")]
        fee: String,
        #[arg(long, default_value_t = false)]
        mainnet: bool,
    },

    #[command(arg_required_else_help = true)]
    Claim {
        stream_id: String,
        #[arg(long, default_value = "~/.local/share/com.rigidnetwork.sage/ssl")]
        cert_path: String,
        #[arg(long, default_value = "0.0001")]
        fee: String,
        #[arg(long, default_value_t = false)]
        mainnet: bool,
    },
}

#[derive(Error, Debug)]
enum CliError {
    #[error("Invalid asset id")]
    InvalidAssetId,
    #[error("Home directory not found")]
    HomeDirectoryNotFound,
    #[error("Sage client error")]
    SageCleint(#[from] client::ClientError),
    #[error("Invalid amount: The amount is in XCH/CAT units, not mojos. Please include a '.' in the amount to indicate that you understand.")]
    InvalidAmount,
    #[error("Invalid address")]
    AddressError(#[from] chia_wallet_sdk::AddressError),
    #[error("Failed to encode address")]
    EncodeAddressError(#[from] bech32::Error),
}

fn expand_tilde<P: AsRef<Path>>(path_str: P) -> Result<PathBuf, CliError> {
    let path = path_str.as_ref();
    if path.starts_with("~") {
        let home = home_dir().ok_or(CliError::HomeDirectoryNotFound)?;
        Ok(home.join(path.strip_prefix("~/").unwrap_or(path)))
    } else {
        Ok(path.to_path_buf())
    }
}

fn parse_amount(amount: String, is_cat: bool) -> Result<u64, CliError> {
    if !amount.contains(".") {
        return Err(CliError::InvalidAmount);
    }

    let Some((whole, fractional)) = amount.split_once('.') else {
        return Err(CliError::InvalidAmount);
    };

    let whole = whole.parse::<u64>().unwrap();
    let fractional = fractional.parse::<u64>().unwrap();

    if is_cat {
        Ok(whole * 1_000 + fractional)
    } else {
        Ok(whole * 1_000_000_000_000_000_000 + fractional)
    }
}

#[tokio::main]
async fn main() -> Result<(), CliError> {
    let args = Cli::parse();

    match args.command {
        Commands::Launch {
            asset_id,
            amount,
            start_timestamp,
            end_timestamp,
            recipient,
            cert_path,
            fee,
            mainnet,
        } => {
            let asset_id = hex::decode(asset_id).map_err(|_| CliError::InvalidAssetId)?;
            let cert_path = expand_tilde(cert_path)?;

            let cert_file = cert_path.join("wallet.crt");
            let key_file = cert_path.join("wallet.key");

            let client =
                SageClient::new(&cert_file, &key_file, "https://localhost:9257".to_string())
                    .map_err(|e| {
                        eprintln!("Failed to create client: {}", e);
                        CliError::HomeDirectoryNotFound
                    })?;

            let (recipient_puzzle_hash, prefix) =
                decode_address(&recipient).map_err(CliError::AddressError)?;
            let cat_amount = parse_amount(amount, true)?;

            let asset_id: [u8; 32] = asset_id.try_into().map_err(|_| CliError::InvalidAssetId)?;
            let target_puzzle_hash = CatArgs::curry_tree_hash(
                Bytes32::from(asset_id),
                StreamPuzzle2ndCurryArgs::curry_tree_hash(
                    Bytes32::new(recipient_puzzle_hash),
                    end_timestamp,
                    start_timestamp,
                ),
            );

            let send_cat_request = SendCat {
                asset_id: hex::encode(asset_id),
                address: encode_address(
                    target_puzzle_hash.into(),
                    if mainnet { "xch" } else { "txch" },
                )
                .map_err(CliError::EncodeAddressError)?,
                amount: Amount(cat_amount),
                fee: Amount(parse_amount(fee, false)?),
                memos: todo!(""),
                auto_submit: true,
            };

            let response = client.send_cat(send_cat_request).await?;
            println!("Response: {:?}", response);
        }
        Commands::View {
            stream_id,
            cert_path,
            fee,
            mainnet,
        } => {
            let cert_path = expand_tilde(cert_path)?;
            println!("Using cert path: {}", cert_path.display());
            println!("Viewing stream with stream_id={stream_id}");
        }
        Commands::Claim {
            stream_id,
            cert_path,
            fee,
            mainnet,
        } => {
            let cert_path = expand_tilde(cert_path)?;
            println!("Using cert path: {}", cert_path.display());
            println!("Claiming stream with stream_id={stream_id}");
        }
    }

    Ok(())
}
