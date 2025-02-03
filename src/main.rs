use chia::{
    bls::PublicKey, consensus::gen::make_aggsig_final_message::u64_to_bytes, traits::Streamable,
};
use chia_protocol::{Bytes, Bytes32, Coin, CoinSpend, Program};
use chia_wallet_sdk::{
    decode_address, encode_address, ChiaRpcClient, CoinsetClient, Conditions, DriverError, Layer,
    Puzzle, SpendContext, StandardLayer,
};
use chrono::{Local, TimeZone};
use clap::{Parser, Subcommand};
use client::SageClient;
use clvm_traits::ToClvm;
use dirs::home_dir;
use sage_api::{
    Amount, AssetKind, CoinJson, CoinSpendJson, GetDerivations, SendCat, SendXch, SignCoinSpends,
};
use std::path::{Path, PathBuf};
use streaming::{StreamPuzzle2ndCurryArgs, StreamedCat};
use thiserror::Error;

mod client;

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
        clawback_address: String,
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
        #[arg(long, default_value_t = false)]
        hardened: bool,
        #[arg(long, default_value = "10000")]
        max_derivations: u64,
    },

    #[command(arg_required_else_help = true)]
    Clawback {
        stream_id: String,
        #[arg(long, default_value = "~/.local/share/com.rigidnetwork.sage/ssl")]
        cert_path: String,
        #[arg(long, default_value = "0.0001")]
        fee: String,
        #[arg(long, default_value_t = false)]
        mainnet: bool,
        #[arg(long, default_value_t = false)]
        hardened: bool,
        #[arg(long, default_value = "10000")]
        max_derivations: u64,
    },
}

#[derive(Error, Debug)]
enum CliError {
    #[error("Invalid asset id")]
    InvalidAssetId,
    #[error("Home directory not found")]
    HomeDirectoryNotFound,
    #[error("Sage client error")]
    SageClient(#[from] client::ClientError),
    #[error("Invalid amount: The amount is in XCH/CAT units, not mojos. Please include a '.' in the amount to indicate that you understand.")]
    InvalidAmount,
    #[error("Invalid address")]
    Address(#[from] chia_wallet_sdk::AddressError),
    #[error("Invalid stream id")]
    InvalidStreamId(),
    #[error("Failed to encode address")]
    EncodeAddress(#[from] bech32::Error),
    #[error("Failed to get streaming coin id - streaming CAT might exist, but the CLI was unable to find it.")]
    UnknownStreamingCoinId,
    #[error("Coinset.org request failed")]
    Reqwest(#[from] reqwest::Error),
    #[error("Driver error")]
    Driver(#[from] chia_wallet_sdk::DriverError),
    #[error("Hex decoding failed")]
    HexDecodingFailed(#[from] hex::FromHexError),
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

    let whole = whole.parse::<u64>().map_err(|_| CliError::InvalidAmount)?;
    let fractional = if is_cat {
        format!("{:0<3}", fractional)
    } else {
        format!("{:0<12}", fractional)
    }
    .parse::<u64>()
    .map_err(|_| CliError::InvalidAmount)?;

    if is_cat {
        // For CATs: 1 CAT = 1000 mojos
        Ok(whole * 1000 + fractional)
    } else {
        // For XCH: 1 XCH = 1_000_000_000_000 mojos
        Ok(whole * 1_000_000_000_000 + fractional)
    }
}

async fn sync_stream(
    stream_id: String,
    cli: &CoinsetClient,
    stream_prefix: &str,
    prefix: &str,
    print: bool,
    print_claimable: bool,
) -> Result<Option<StreamedCat>, CliError> {
    println!("Viewing stream with id {stream_id}");

    let (stream_coin_id, decoded_stream_prefix) =
        decode_address(&stream_id).map_err(|_| CliError::InvalidStreamId())?;
    if decoded_stream_prefix != stream_prefix {
        return Err(CliError::InvalidStreamId());
    }
    let stream_coin_id = Bytes32::from(stream_coin_id);

    let mut first_run = true;
    let mut ctx = SpendContext::new();
    let mut latest_coin_id = stream_coin_id;
    let mut latest_stream = None;

    loop {
        let coin_record_resp = cli
            .get_coin_record_by_name(latest_coin_id)
            .await
            .map_err(CliError::Reqwest)?;

        if !coin_record_resp.success {
            println!("Failed to get coin record :(");
            return Ok(None);
        }

        let Some(coin_record) = coin_record_resp.coin_record else {
            println!("Coin record not available");
            return Ok(None);
        };

        if first_run {
            // Parse parent spend to get first stream
            latest_coin_id = coin_record.coin.parent_coin_info;
            first_run = false;
            continue;
        }

        if coin_record.spent_block_index == 0 {
            if print {
                println!(
                    "  Coin {} currently unspent.",
                    hex::encode(latest_coin_id.to_vec())
                );
            }
            break;
        }

        let puzzle_and_solution = cli
            .get_puzzle_and_solution(
                coin_record.coin.coin_id(),
                Some(coin_record.spent_block_index),
            )
            .await
            .map_err(CliError::Reqwest)?;
        let Some(coin_solution) = puzzle_and_solution.coin_solution else {
            println!("Failed to get puzzle and solution");
            return Ok(None);
        };

        let parent_puzzle = coin_solution
            .puzzle_reveal
            .to_clvm(&mut ctx.allocator)
            .map_err(|e| CliError::Driver(DriverError::ToClvm(e)))?;
        let parent_solution = coin_solution
            .solution
            .to_clvm(&mut ctx.allocator)
            .map_err(|e| CliError::Driver(DriverError::ToClvm(e)))?;
        let parent_puzzle = Puzzle::parse(&ctx.allocator, parent_puzzle);

        let (new_stream, clawbacked, paid_amount_if_clawback) = StreamedCat::from_parent_spend(
            &mut ctx.allocator,
            coin_record.coin,
            parent_puzzle,
            parent_solution,
        )?;
        let Some(new_stream) = new_stream else {
            if clawbacked {
                if print {
                    println!(
                        "  Streamed CAT was clawed back; last payment was {:.3} CATs.",
                        paid_amount_if_clawback as f64 / 1000.0
                    );
                }
            } else {
                println!("Failed to parse streamed CAT");
            }
            return Ok(None);
        };

        if latest_stream.is_none() && print {
            println!("Asset id: {}", hex::encode(new_stream.asset_id.to_vec()));
            println!(
                "Total amount: {:.3}",
                new_stream.coin.amount as f64 / 1000.0
            );
            println!(
                "Recipient address: {}",
                encode_address(new_stream.recipient.into(), prefix).unwrap()
            );
            println!(
                "Clawback address: {}",
                encode_address(new_stream.clawback_ph.into(), prefix).unwrap()
            );
            println!(
                "Start time: {} (local: {})",
                new_stream.last_payment_time,
                Local
                    .timestamp_opt(new_stream.last_payment_time as i64, 0)
                    .unwrap()
                    .format("%Y-%m-%d %H:%M:%S")
            );
            println!(
                "End time: {} (local: {})",
                new_stream.end_time,
                Local
                    .timestamp_opt(new_stream.end_time as i64, 0)
                    .unwrap()
                    .format("%Y-%m-%d %H:%M:%S")
            );
            println!("Spends:");
        } else if print {
            println!(
                "  Coin {} spent at block {} to claim {} CATs.",
                hex::encode(latest_coin_id.to_vec()),
                coin_record.spent_block_index,
                (coin_record.coin.amount - new_stream.coin.amount) as f64 / 1000.0
            );
        }

        latest_coin_id = new_stream.coin.coin_id();
        latest_stream = Some(new_stream);
    }

    if print {
        if let Some(latest_stream) = latest_stream {
            println!(
                "Remaining (unclaimed) amount: {:.3}",
                latest_stream.coin.amount as f64 / 1000.0
            );
            println!(
                "Latest claim time: {} (local: {})",
                latest_stream.last_payment_time,
                Local
                    .timestamp_opt(latest_stream.last_payment_time as i64, 0)
                    .unwrap()
                    .format("%Y-%m-%d %H:%M:%S")
            );

            if print_claimable {
                let time_now = get_latest_timestamp(cli).await?;
                let claimable = latest_stream.amount_to_be_paid(time_now);
                println!("Claimable right now: {:.3} CATs", claimable as f64 / 1000.0);
            }

            return Ok(Some(latest_stream));
        }
    }

    Ok(latest_stream)
}

async fn wait_for_coin(
    coin_id: Bytes32,
    cli: &CoinsetClient,
    also_check_for_spent: bool,
) -> Result<(), CliError> {
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

        let coin_resp = cli.get_coin_record_by_name(coin_id).await?;

        if coin_resp.success && coin_resp.coin_record.is_some() {
            if also_check_for_spent {
                if let Some(coin_record) = coin_resp.coin_record {
                    if coin_record.spent {
                        break;
                    }
                }
            } else {
                break;
            }
        }
    }

    Ok(())
}

async fn get_latest_timestamp(cli: &CoinsetClient) -> Result<u64, CliError> {
    let state_resp = cli
        .get_blockchain_state()
        .await
        .map_err(CliError::Reqwest)?;
    let Some(state) = state_resp.blockchain_state else {
        println!("Failed to get blockchain state");
        return Err(CliError::InvalidStreamId());
    };

    let mut block_record = state.peak;
    while block_record.timestamp.is_none() {
        let block_resp = cli
            .get_block_record_by_height(block_record.height - 1)
            .await
            .map_err(CliError::Reqwest)?;
        let Some(new_block_record) = block_resp.block_record else {
            println!("Failed to get block record");
            return Err(CliError::InvalidStreamId());
        };

        block_record = new_block_record;
    }

    Ok(block_record.timestamp.unwrap())
}

async fn get_public_key(
    cli: &SageClient,
    address: &str,
    max_derivations: u64,
    hardened: bool,
) -> Result<PublicKey, CliError> {
    let mut public_key: Option<PublicKey> = None;
    for i in (0..max_derivations).step_by(1000) {
        let derivation_resp = cli
            .get_derivations(GetDerivations {
                offset: i as u32,
                limit: 1000,
                hardened,
            })
            .await?;

        for derivation in derivation_resp.derivations {
            if derivation.address == address {
                let pubkey_bytes = hex::decode(derivation.public_key).unwrap();
                let pubkey_bytes: [u8; 48] = pubkey_bytes.try_into().unwrap();
                public_key = Some(PublicKey::from_bytes(&pubkey_bytes).unwrap());
                break;
            }
        }
    }

    let Some(public_key) = public_key else {
        println!("Failed to find public key");
        return Err(CliError::InvalidStreamId());
    };

    Ok(public_key)
}

#[allow(clippy::too_many_arguments)]
async fn generate_spend_bundle(
    sage_client: &SageClient,
    latest_streamed_coin: StreamedCat,
    public_key: PublicKey,
    p2_puzzle_hash: Bytes32,
    p2_address: &str,
    fee: String,
    claim_time: u64,
    clawback: bool,
) -> Result<Bytes32, CliError> {
    let mut ctx = SpendContext::new();
    let p2 = StandardLayer::new(public_key);
    let p2_puzzle_ptr = p2.construct_puzzle(&mut ctx)?;
    if ctx.tree_hash(p2_puzzle_ptr) != p2_puzzle_hash.into() {
        eprintln!("Wallet is using non-standard puzzle :(");
        return Err(CliError::InvalidStreamId());
    }

    let initial_send = sage_client
        .send_xch(SendXch {
            address: p2_address.to_string(),
            amount: Amount::Number(0),
            fee: Amount::Number(parse_amount(fee, false)?),
            memos: vec![],
            auto_submit: false,
        })
        .await?;

    for spend in initial_send.coin_spends {
        let parent_coin_info: [u8; 32] = hex::decode(spend.coin.parent_coin_info.replace("0x", ""))
            .map_err(CliError::HexDecodingFailed)?
            .try_into()
            .unwrap();
        let puzzle_hash: [u8; 32] = hex::decode(spend.coin.puzzle_hash.replace("0x", ""))
            .map_err(CliError::HexDecodingFailed)?
            .try_into()
            .unwrap();
        let coin = Coin::new(
            Bytes32::from(parent_coin_info),
            Bytes32::from(puzzle_hash),
            match spend.coin.amount {
                Amount::Number(amount) => amount,
                Amount::String(amount) => amount.parse::<u64>().unwrap(),
            },
        );

        let puzzle_reveal: Vec<u8> = hex::decode(spend.puzzle_reveal.replace("0x", "0"))
            .map_err(CliError::HexDecodingFailed)?;
        let solution: Vec<u8> =
            hex::decode(spend.solution.replace("0x", "0")).map_err(CliError::HexDecodingFailed)?;

        ctx.insert(CoinSpend {
            coin,
            puzzle_reveal: Program::from_bytes(&puzzle_reveal).unwrap(),
            solution: Program::from_bytes(&solution).unwrap(),
        });
    }

    let mut lead_coin_parent: Option<Bytes32> = None;
    for input in initial_send.summary.inputs {
        let AssetKind::Xch = input.kind else {
            continue;
        };

        if !input
            .outputs
            .iter()
            .any(|c| c.amount == Amount::Number(0) && c.address == p2_address)
        {
            continue;
        };

        let lead_coin_parent_b32: [u8; 32] = hex::decode(input.coin_id.replace("0x", ""))?
            .try_into()
            .unwrap();
        lead_coin_parent = Some(Bytes32::from(lead_coin_parent_b32));
    }

    let Some(lead_coin_parent) = lead_coin_parent else {
        println!("Failed to find lead coin parent");
        return Err(CliError::InvalidStreamId());
    };

    let lead_coin = Coin::new(lead_coin_parent, p2_puzzle_hash, 0);

    let message_to_send = Bytes::new(u64_to_bytes(claim_time));
    let coin_id_ptr = latest_streamed_coin
        .coin
        .coin_id()
        .to_clvm(&mut ctx.allocator)
        .map_err(|e| CliError::Driver(DriverError::ToClvm(e)))?;
    p2.spend(
        &mut ctx,
        lead_coin,
        Conditions::new().send_message(23, message_to_send, vec![coin_id_ptr]),
    )?;
    latest_streamed_coin.spend(&mut ctx, claim_time, clawback)?;

    println!("Spend bundle ready. Last confirmation - press 'Enter' to proceed");
    let _ = std::io::stdin().read_line(&mut String::new());

    let sign_request = SignCoinSpends {
        coin_spends: ctx
            .take()
            .iter()
            .map(|c| CoinSpendJson {
                coin: CoinJson {
                    parent_coin_info: format!(
                        "0x{}",
                        hex::encode(c.coin.parent_coin_info.to_vec())
                    ),
                    puzzle_hash: format!("0x{}", hex::encode(c.coin.puzzle_hash.to_vec())),
                    amount: Amount::Number(c.coin.amount),
                },
                puzzle_reveal: format!("0x{}", hex::encode(c.puzzle_reveal.to_vec())),
                solution: format!("0x{}", hex::encode(c.solution.to_vec())),
            })
            .collect(),
        auto_submit: true,
        partial: false,
    };

    let _ = sage_client.sign_coin_spends(sign_request).await?;

    Ok(latest_streamed_coin.coin.coin_id())
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
            clawback_address,
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
                decode_address(&recipient).map_err(CliError::Address)?;
            let (clawback_ph, _prefix) =
                decode_address(&clawback_address).map_err(CliError::Address)?;
            let cat_amount = parse_amount(amount, true)?;

            let asset_id: [u8; 32] = asset_id.try_into().map_err(|_| CliError::InvalidAssetId)?;
            let target_inner_puzzle_hash = StreamPuzzle2ndCurryArgs::curry_tree_hash(
                Bytes32::new(recipient_puzzle_hash),
                clawback_ph.into(),
                end_timestamp,
                start_timestamp,
            );

            println!("You're about to start streaming a CAT to {}", recipient);
            println!("Note: Sage RPC should be running on port 9257\n");
            println!("Please note that the CAT can only be clawed back by the clawback address. Please ensure the details below are correct.");
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
                target_inner_puzzle_hash.into(),
                if mainnet { "xch" } else { "txch" },
            )
            .map_err(CliError::EncodeAddress)?;

            println!("Sending CAT...");
            let send_cat_request = SendCat {
                asset_id: hex::encode(asset_id),
                address: streaming_cat_address.clone(),
                amount: Amount::Number(cat_amount),
                fee: Amount::Number(parse_amount(fee, false)?),
                memos: StreamedCat::get_launch_hints(
                    Bytes32::new(recipient_puzzle_hash),
                    clawback_ph.into(),
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
            let actual_asset_id = asset_id;
            for coin in response.summary.inputs {
                if let AssetKind::Cat { asset_id, .. } = coin.kind {
                    if asset_id.replace("0x", "") != hex::encode(actual_asset_id) {
                        continue;
                    }
                } else {
                    continue;
                }

                for output in coin.outputs {
                    if !output.receiving && output.address == streaming_cat_address {
                        streaming_coin_id = Some(output.coin_id.clone());
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
                encode_address(
                    streaming_coin_id,
                    if mainnet { "stream" } else { "tstream" }
                )
                .unwrap()
            );

            println!("Waiting for mempool item to be confirmed...");
            let cli = if mainnet {
                CoinsetClient::mainnet()
            } else {
                CoinsetClient::testnet11()
            };

            wait_for_coin(streaming_coin_id.into(), &cli, false).await?;
            println!("Confimed! :)");
        }
        Commands::View { stream_id, mainnet } => {
            let cli = if mainnet {
                CoinsetClient::mainnet()
            } else {
                CoinsetClient::testnet11()
            };
            let stream_prefix = if mainnet { "stream" } else { "tstream" };
            let prefix = if mainnet { "xch" } else { "txch" };
            let _ = sync_stream(stream_id, &cli, stream_prefix, prefix, true, true).await?;
        }
        Commands::Claim {
            stream_id,
            cert_path,
            fee,
            mainnet,
            hardened,
            max_derivations,
        } => {
            let cert_path = expand_tilde(cert_path)?;

            let cli = if mainnet {
                CoinsetClient::mainnet()
            } else {
                CoinsetClient::testnet11()
            };

            println!("Fetching latest unspent coin...");

            let latest_streamed_coin = sync_stream(
                stream_id,
                &cli,
                if mainnet { "stream" } else { "tstream" },
                if mainnet { "xch" } else { "txch" },
                true,
                false,
            )
            .await?
            .unwrap();

            let latest_timestamp = get_latest_timestamp(&cli).await?;

            println!("Latest block timestamp: {}", latest_timestamp);
            let claim_time = if latest_timestamp - 1 <= latest_streamed_coin.end_time {
                latest_timestamp - 1
            } else {
                latest_streamed_coin.end_time
            };
            let claim_amount = latest_streamed_coin.amount_to_be_paid(claim_time);

            println!("Claim amount: {:.3} CATs", claim_amount as f64 / 1000.0);
            println!("Press 'Enter' to proceed");
            let _ = std::io::stdin().read_line(&mut String::new());

            let recipient = latest_streamed_coin.recipient;
            let recipient_address =
                encode_address(recipient.into(), if mainnet { "xch" } else { "txch" }).map_err(
                    |e| {
                        eprintln!("Failed to encode address: {}", e);
                        CliError::InvalidStreamId()
                    },
                )?;
            println!(
                "Searching for key associated with address: {}",
                recipient_address
            );

            let cert_file = cert_path.join("wallet.crt");
            let key_file = cert_path.join("wallet.key");

            let sage_client =
                SageClient::new(&cert_file, &key_file, "https://localhost:9257".to_string())
                    .map_err(|e| {
                        eprintln!("Failed to create Sage client: {}", e);
                        CliError::HomeDirectoryNotFound
                    })?;
            let public_key =
                get_public_key(&sage_client, &recipient_address, max_derivations, hardened).await?;

            println!("Building spend bundle...");
            let coin_id = generate_spend_bundle(
                &sage_client,
                latest_streamed_coin,
                public_key,
                recipient,
                &recipient_address,
                fee,
                claim_time,
                false,
            )
            .await?;

            println!("Waiting for transaction to be confirmed...");
            wait_for_coin(coin_id, &cli, true).await?;
            println!("Confirmed :)");
        }
        Commands::Clawback {
            stream_id,
            cert_path,
            fee,
            mainnet,
            hardened,
            max_derivations,
        } => {
            let cert_path = expand_tilde(cert_path)?;

            let cli = if mainnet {
                CoinsetClient::mainnet()
            } else {
                CoinsetClient::testnet11()
            };

            println!("Fetching latest unspent coin...");

            let latest_streamed_coin = sync_stream(
                stream_id,
                &cli,
                if mainnet { "stream" } else { "tstream" },
                if mainnet { "xch" } else { "txch" },
                true,
                false,
            )
            .await?
            .unwrap();

            let latest_timestamp = get_latest_timestamp(&cli).await?;

            println!("Latest block timestamp: {}", latest_timestamp);
            let claim_time = if latest_timestamp + 600 <= latest_streamed_coin.end_time {
                latest_timestamp + 600
            } else {
                latest_streamed_coin.end_time
            };
            let claim_amount = latest_streamed_coin.amount_to_be_paid(claim_time);

            println!(
                "Approx. claim amount: {:.3} CATs; Approx. return amount: {:.3} CATs",
                claim_amount as f64 / 1000.0,
                (latest_streamed_coin.coin.amount - claim_amount) as f64 / 1000.0
            );
            println!("Press 'Enter' to proceed");
            let _ = std::io::stdin().read_line(&mut String::new());

            let clawback_ph = latest_streamed_coin.clawback_ph;
            let clawback_address =
                encode_address(clawback_ph.into(), if mainnet { "xch" } else { "txch" }).map_err(
                    |e| {
                        eprintln!("Failed to encode address: {}", e);
                        CliError::InvalidStreamId()
                    },
                )?;
            println!(
                "Searching for key associated with address: {}",
                clawback_address
            );

            let cert_file = cert_path.join("wallet.crt");
            let key_file = cert_path.join("wallet.key");

            let sage_client =
                SageClient::new(&cert_file, &key_file, "https://localhost:9257".to_string())
                    .map_err(|e| {
                        eprintln!("Failed to create Sage client: {}", e);
                        CliError::HomeDirectoryNotFound
                    })?;
            let public_key =
                get_public_key(&sage_client, &clawback_address, max_derivations, hardened).await?;

            println!("Building spend bundle...");
            let coin_id = generate_spend_bundle(
                &sage_client,
                latest_streamed_coin.clone(),
                public_key,
                clawback_ph,
                &clawback_address,
                fee.clone(),
                claim_time,
                true,
            )
            .await?;

            println!("Waiting for transaction to be confirmed...");
            wait_for_coin(coin_id, &cli, true).await?;
            println!("Confirmed :)");
        }
    }

    Ok(())
}
