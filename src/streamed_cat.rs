use chia::puzzles::{cat::CatSolution, CoinProof, LineageProof};
use chia_protocol::{Bytes32, Coin};
use chia_wallet_sdk::{CatLayer, DriverError, Layer, Spend, SpendContext};
use clvmr::NodePtr;

use crate::{StreamLayer, StreamPuzzleSolution};

#[derive(Debug, Clone)]
#[must_use]
pub struct StreamedCat {
    pub coin: Coin,
    pub asset_id: Bytes32,
    pub proof: LineageProof,
    pub inner_puzzle_hash: Bytes32,

    pub recipient: Bytes32,
    pub end_time: u64,
    pub last_payment_time: u64,
}

impl StreamedCat {
    pub fn new(
        coin: Coin,
        asset_id: Bytes32,
        proof: LineageProof,
        inner_puzzle_hash: Bytes32,
        recipient: Bytes32,
        end_time: u64,
        last_payment_time: u64,
    ) -> Self {
        Self {
            coin,
            asset_id,
            proof,
            inner_puzzle_hash,
            recipient,
            end_time,
            last_payment_time,
        }
    }

    pub fn layers(&self) -> CatLayer<StreamLayer> {
        CatLayer::<StreamLayer>::new(
            self.asset_id,
            StreamLayer::new(self.recipient, self.end_time, self.last_payment_time),
        )
    }

    pub fn construct_puzzle(&self, ctx: &mut SpendContext) -> Result<NodePtr, DriverError> {
        self.layers().construct_puzzle(ctx)
    }

    pub fn construct_solution(
        &self,
        ctx: &mut SpendContext,
        payment_time: u64,
    ) -> Result<NodePtr, DriverError> {
        self.layers().construct_solution(
            ctx,
            CatSolution {
                inner_puzzle_solution: StreamPuzzleSolution {
                    my_amount: self.coin.amount,
                    payment_time,
                },
                lineage_proof: Some(self.proof),
                prev_coin_id: self.coin.coin_id(),
                this_coin_info: self.coin,
                next_coin_proof: CoinProof {
                    parent_coin_info: self.coin.parent_coin_info,
                    inner_puzzle_hash: self.inner_puzzle_hash,
                    amount: self.coin.amount,
                },
                prev_subtotal: 0,
                extra_delta: 0,
            },
        )
    }

    pub fn spend(&self, ctx: &mut SpendContext, payment_time: u64) -> Result<(), DriverError> {
        let puzzle = self.construct_puzzle(ctx)?;
        let solution = self.construct_solution(ctx, payment_time)?;

        ctx.spend(self.coin, Spend::new(puzzle, solution))
    }
}
