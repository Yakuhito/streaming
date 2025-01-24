use chia::puzzles::{
    cat::{CatArgs, CatSolution},
    CoinProof, LineageProof,
};
use chia_protocol::{Bytes32, Coin};
use chia_wallet_sdk::{CatLayer, DriverError, Layer, Puzzle, Spend, SpendContext};
use clvm_traits::FromClvm;
use clvmr::{Allocator, NodePtr};

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
        recipient: Bytes32,
        end_time: u64,
        last_payment_time: u64,
    ) -> Self {
        Self {
            coin,
            asset_id,
            proof,
            inner_puzzle_hash: StreamLayer::new(recipient, end_time, last_payment_time)
                .puzzle_hash()
                .into(),
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

    pub fn from_parent_spend(
        allocator: &mut Allocator,
        parent_coin: Coin,
        parent_puzzle: Puzzle,
        parent_solution: NodePtr,
    ) -> Result<Option<Self>, DriverError> {
        let Some(layers) = CatLayer::<StreamLayer>::parse_puzzle(allocator, parent_puzzle)? else {
            return Ok(None);
        };

        let proof = LineageProof {
            parent_parent_coin_info: parent_coin.parent_coin_info,
            parent_inner_puzzle_hash: layers.inner_puzzle.puzzle_hash().into(),
            parent_amount: parent_coin.amount,
        };

        let parent_solution =
            CatSolution::<StreamPuzzleSolution>::from_clvm(allocator, parent_solution)?;

        let new_amount = parent_coin.amount
            * (parent_solution.inner_puzzle_solution.payment_time
                - layers.inner_puzzle.last_payment_time)
            / (layers.inner_puzzle.end_time - layers.inner_puzzle.last_payment_time);

        let new_inner_layer = StreamLayer::new(
            layers.inner_puzzle.recipient,
            layers.inner_puzzle.end_time,
            parent_solution.inner_puzzle_solution.payment_time,
        );
        let new_puzzle_hash =
            CatArgs::curry_tree_hash(layers.asset_id, new_inner_layer.puzzle_hash());

        Ok(Some(Self::new(
            Coin::new(parent_coin.coin_id(), new_puzzle_hash.into(), new_amount),
            layers.asset_id,
            proof,
            layers.inner_puzzle.recipient,
            layers.inner_puzzle.end_time,
            // last payment time should've been updated by the spend
            parent_solution.inner_puzzle_solution.payment_time,
        )))
    }
}
