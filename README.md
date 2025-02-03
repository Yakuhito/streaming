# Streaming
Streaming puzzle for Chia CATs

## Testing

This repository contains a CLI that can be used to test the streaming puzzle on testnet with Sage. Rust is required to proceed.

To start, open the Sage CLI and switch to 'testnet11' in settings. After getting some TXCH, create a new CAT and note the asset id. Also navigate to 'Addresses' and copy your first two addresses - the first one will be the recipient (address that receives streamed CATs), while the second address will be used for clawbacks.

With the 3 strings noted somewhere (asset id, first address, second address), close the CLI without logging out and start the Sage RPC:

```bash
cargo install --git https://github.com/xch-dev/sage sage-cli
sage rpc start
```

Note that the Sage RPC should never run at the same time as the Sage UI. Change directory to this repository and run the following command to launch a streaming CAT:

```bash
cargo r --release launch <ASSET_ID> <AMOUNT> <START_TIMESTAMP> <END_TIMESTAMP> <RECIPIENT=FIRST ADDRESS> <CLAWBACK_ADDRESS=SECOND ADDRESS> --fee <FEE>
```

Note: The default fee is 0.0001 TXCH.

The start timestamp could be the current one, which is easily obtainable via websites such as [this one](https://www.unixtimestamp.com/). You can get the end timestamp by taking the start timestamp and adding the number of seconds the streaming period has - if you want to test streaming over 24 hours, for example, add `24 * 60 * 60 = 86400` seconds. Also note the amount is in full CAT units, not mojos - so '1.2' means 1.2 CATs or 1200 mojos. To prevent confusion, you are required to include a '.' in the amount. So, if you want to stream 24 CATs, the amount should be '24.'.

Make note of the stream id, which is the streamed CAT's unique identifier. It should start with 'ts1' on testnet (and 's1' on mainnet).

To view the streamed CAT status and history at any point, you can use the following command:

```bash
cargo r --release view <STREAM_ID>
```

To get the claimable CAT, the recipient can use the following command:

```bash
cargo r --release claim <STREAM_ID> --fee <FEE>
```

Note: The default fee is 0.0001 TXCH.

Lastly, if the clawback address owner wants to stop streaming, they can use the following command:

```bash
cargo r --release clawback <STREAM_ID> --fee <FEE>
```

Note: The default fee is 0.0001 TXCH.

Clawbacks pay the claimable amount to the recipient - they only return the amount of CAT that would've been distributed in the future.
