use chia::puzzles::cat::CatArgs;
use chia_protocol::Bytes32;
use chia_wallet_sdk::{decode_address, encode_address, ChiaRpcClient, CoinsetClient};
use chrono::{Local, TimeZone};
use clap::{Parser, Subcommand};
use dirs::home_dir;
use std::path::{Path, PathBuf};
use streaming::{StreamPuzzle2ndCurryArgs, StreamedCat};
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
    #[error("Failed to get streaming coin id - streaming CAT might exist, but the CLI was unable to find it.")]
    UnknownStreamingCoinId,
    #[error("Coinset.org request failed")]
    ReqwestError(#[from] reqwest::Error),
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

            let (recipient_puzzle_hash, _prefix) =
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

            println!("You're about to start streaming a CAT to {}", recipient);
            println!("Note: Sage RPC should be running on port 9257\n");
            println!("Please note that the CAT CANNOT be clawed back. Please ensure the details below are correct.");
            println!("Asset ID: {}", hex::encode(asset_id));
            println!("Amount: {:.3}", cat_amount as f64 / 1000.0);
            println!(
                "Start Time: {}",
                Local
                    .timestamp_opt(start_timestamp as i64, 0)
                    .unwrap()
                    .format("%Y-%m-%d %H:%M:%S")
            );
            println!(
                "End Time: {}",
                Local
                    .timestamp_opt(end_timestamp as i64, 0)
                    .unwrap()
                    .format("%Y-%m-%d %H:%M:%S")
            );
            println!(
                "Fee: {:.12}",
                parse_amount(fee.clone(), false)? as f64 / 1_000_000_000_000.0
            );
            println!("Mainnet?: {}", mainnet);

            println!("Press Enter to continue...");
            let _ = std::io::stdin().read_line(&mut String::new());

            let streaming_cat_address = encode_address(
                target_puzzle_hash.into(),
                if mainnet { "xch" } else { "txch" },
            )
            .map_err(CliError::EncodeAddressError)?;

            println!("Sending CAT...");
            let send_cat_request = SendCat {
                asset_id: hex::encode(asset_id),
                address: streaming_cat_address.clone(),
                amount: Amount(cat_amount),
                fee: Amount(parse_amount(fee, false)?),
                memos: StreamedCat::get_launch_hints(
                    Bytes32::new(recipient_puzzle_hash),
                    start_timestamp,
                    end_timestamp,
                )
                .iter()
                .map(|b| hex::encode(b.to_vec()))
                .collect(),
                auto_submit: true,
            };

            let response = client.send_cat(send_cat_request).await?;

            let mut streaming_coin_id: Option<String> = None;
            for coin in response.summary.inputs {
                if coin.coin_type != Some("cat".to_string())
                    || coin.asset_id != Some(hex::encode(asset_id))
                {
                    continue;
                }

                for output in coin.outputs {
                    if !output.receiving && output.address == streaming_cat_address {
                        streaming_coin_id = Some(coin.coin_id.clone());
                        break;
                    }
                }

                if streaming_coin_id.is_some() {
                    break;
                }
            }

            let Some(streaming_coin_id) = streaming_coin_id else {
                return Err(CliError::UnknownStreamingCoinId);
            };

            println!("Streaming coin id: 0x{}", streaming_coin_id);

            let streaming_coin_id = hex::decode(streaming_coin_id)
                .map_err(|_| CliError::UnknownStreamingCoinId)?
                .try_into()
                .map_err(|_| CliError::UnknownStreamingCoinId)?;
            println!(
                "Stream id: {}",
                encode_address(streaming_coin_id, "s").unwrap()
            );

            println!("Waiting for mempool item to be confirmed...");
            let cli = if mainnet {
                CoinsetClient::mainnet()
            } else {
                CoinsetClient::testnet11()
            };

            loop {
                let resp = cli
                    .get_coin_record_by_name(streaming_coin_id.into())
                    .await
                    .map_err(CliError::ReqwestError)?;

                if resp.success && resp.coin_record.is_some() {
                    break;
                }

                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            }

            println!("Confimed! :)");
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
