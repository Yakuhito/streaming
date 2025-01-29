use chia::puzzles::{
    cat::{CatArgs, CatSolution},
    CoinProof, LineageProof,
};
use chia_protocol::{Bytes32, Coin};
use chia_wallet_sdk::{CatLayer, DriverError, Layer, Puzzle, Spend, SpendContext};
use clvm_traits::FromClvm;
use clvmr::{reduction::Reduction, Allocator, NodePtr};

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

    pub fn amount_to_be_paid(&self, payment_time: u64) -> u64 {
        // LAST_PAYMENT_TIME + (to_pay * (END_TIME - LAST_PAYMENT_TIME) / my_amount) = payment_time
        // to_pay = my_amount * (payment_time - LAST_PAYMENT_TIME) / (END_TIME - LAST_PAYMENT_TIME)
        self.coin.amount * (payment_time - self.last_payment_time)
            / (self.end_time - self.last_payment_time)
    }

    pub fn construct_solution(
        &self,
        ctx: &mut SpendContext,
        to_pay: u64,
    ) -> Result<NodePtr, DriverError> {
        self.layers().construct_solution(
            ctx,
            CatSolution {
                inner_puzzle_solution: StreamPuzzleSolution {
                    my_amount: self.coin.amount,
                    to_pay,
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

    pub fn spend(&self, ctx: &mut SpendContext, to_pay: u64) -> Result<(), DriverError> {
        let puzzle = self.construct_puzzle(ctx)?;
        let solution = self.construct_solution(ctx, to_pay)?;

        let Reduction(cost, _output) = clvmr::run_program(
            &mut ctx.allocator,
            &clvmr::ChiaDialect::new(0),
            puzzle,
            solution,
            11_000_000_000,
        )?;
        println!("cost: {cost}");
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

        let paid_amount = parent_solution.inner_puzzle_solution.to_pay;
        let payment_time = layers.inner_puzzle.last_payment_time
            + (paid_amount
                * (layers.inner_puzzle.end_time - layers.inner_puzzle.last_payment_time)
                / parent_coin.amount);
        let paid_amount = parent_coin.amount
            * (payment_time - layers.inner_puzzle.last_payment_time)
            / (layers.inner_puzzle.end_time - layers.inner_puzzle.last_payment_time);
        let new_amount = parent_coin.amount - paid_amount;

        let new_inner_layer = StreamLayer::new(
            layers.inner_puzzle.recipient,
            layers.inner_puzzle.end_time,
            payment_time,
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
            payment_time,
        )))
    }
}

#[cfg(test)]
mod tests {
    use chia::puzzles::standard::StandardArgs;
    use chia_protocol::Bytes;
    use chia_wallet_sdk::{test_secret_key, Cat, Conditions, Simulator, StandardLayer};
    use clvm_traits::ToClvm;
    use clvm_utils::tree_hash;
    use clvmr::serde::node_from_bytes;

    use crate::{STREAM_PUZZLE, STREAM_PUZZLE_HASH};

    use super::*;

    #[test]
    fn test_puzzle_hash() {
        let mut allocator = Allocator::new();

        let ptr = node_from_bytes(&mut allocator, &STREAM_PUZZLE).unwrap();
        assert_eq!(tree_hash(&allocator, ptr), STREAM_PUZZLE_HASH);
    }

    #[test]
    fn test_streamed_cat() -> anyhow::Result<()> {
        let ctx = &mut SpendContext::new();
        let mut sim = Simulator::new();

        let claim_intervals = [1000, 2000, 500, 1000, 10];
        let total_claim_time = claim_intervals.iter().sum::<u64>();

        // Create CAT & launch vesting one
        let user_sk = test_secret_key()?;
        let user_p2 = StandardLayer::new(user_sk.public_key());
        let user_puzzle_hash: Bytes32 = StandardArgs::curry_tree_hash(user_sk.public_key()).into();

        let payment_cat_amount = 1000;
        let (minter_sk, minter_pk, _minter_puzzle_hash, minter_coin) =
            sim.new_p2(payment_cat_amount)?;
        let minter_p2 = StandardLayer::new(minter_pk);

        let streaming_inner_puzzle =
            StreamLayer::new(user_puzzle_hash, total_claim_time + 1000, 1000);
        let streaming_inner_puzzle_hash: Bytes32 = streaming_inner_puzzle.puzzle_hash().into();
        let (issue_cat, eve_cat) = Cat::single_issuance_eve(
            ctx,
            minter_coin.coin_id(),
            payment_cat_amount,
            Conditions::new().create_coin(streaming_inner_puzzle_hash, payment_cat_amount, None),
        )?;
        minter_p2.spend(ctx, minter_coin, issue_cat)?;

        let initial_vesting_cat =
            eve_cat.wrapped_child(streaming_inner_puzzle_hash, payment_cat_amount);
        sim.spend_coins(ctx.take(), &[minter_sk.clone()])?;
        sim.set_next_timestamp(1000 + claim_intervals[0])?;

        // spend streaming CAT
        let mut streamed_cat = StreamedCat::new(
            initial_vesting_cat.coin,
            initial_vesting_cat.asset_id,
            initial_vesting_cat.lineage_proof.unwrap(),
            user_puzzle_hash,
            total_claim_time + 1000,
            1000,
        );

        let mut claim_time = sim.next_timestamp();
        for (i, _interval) in claim_intervals.iter().enumerate() {
            /* Payment is always based on last block's timestamp */
            if i < claim_intervals.len() - 1 {
                sim.pass_time(claim_intervals[i + 1]);
            }

            let to_pay = streamed_cat.amount_to_be_paid(claim_time);
            println!("to_pay: {}", to_pay);
            // to claim the payment, user needs to send a message to the streaming CAT
            let user_coin = sim.new_coin(user_puzzle_hash, 0);
            let message_to_send = format!("{:x}", to_pay);
            let message_to_send = Bytes::from(hex::decode(if message_to_send.len() % 2 == 0 {
                message_to_send
            } else {
                format!("0{:x}", to_pay)
            })?);
            let coin_id_ptr = streamed_cat.coin.coin_id().to_clvm(&mut ctx.allocator)?;
            user_p2.spend(
                ctx,
                user_coin,
                Conditions::new().send_message(23, message_to_send, vec![coin_id_ptr]),
            )?;

            streamed_cat.spend(ctx, to_pay)?;

            let spends = ctx.take();
            let streamed_cat_spend = spends.last().unwrap().clone();
            sim.spend_coins(spends, &[user_sk.clone()])?;

            // set up for next iteration
            if i < claim_intervals.len() - 1 {
                claim_time += claim_intervals[i + 1];
            }
            let parent_puzzle = streamed_cat_spend
                .puzzle_reveal
                .to_clvm(&mut ctx.allocator)?;
            let parent_puzzle = Puzzle::from_clvm(&ctx.allocator, parent_puzzle)?;
            let parent_solution = streamed_cat_spend.solution.to_clvm(&mut ctx.allocator)?;
            streamed_cat = StreamedCat::from_parent_spend(
                &mut ctx.allocator,
                streamed_cat.coin,
                parent_puzzle,
                parent_solution,
            )?
            .unwrap();
        }

        Ok(())
    }
}
