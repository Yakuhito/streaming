use clap::{Parser, Subcommand};

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
        start_time: u64,
        end_time: u64,
        recipient: String,
    },

    #[command(arg_required_else_help = true)]
    View { stream_id: String },

    #[command(arg_required_else_help = true)]
    Claim { stream_id: String },
}

fn main() {
    let args = Cli::parse();

    match args.command {
        Commands::Launch {
            asset_id,
            amount,
            start_time,
            end_time,
            recipient,
        } => {
            println!("Launching stream with asset_id={asset_id}, amount={amount}, start_time={start_time}, end_time={end_time}, recipient={recipient}");
        }
        Commands::View { stream_id } => {
            println!("Viewing stream with stream_id={stream_id}");
        }
        Commands::Claim { stream_id } => {
            println!("Claiming stream with stream_id={stream_id}");
        }
    }
}
