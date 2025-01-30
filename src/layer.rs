use chia_protocol::Bytes32;
use chia_wallet_sdk::{CurriedPuzzle, DriverError, Layer, Mod, Puzzle, SpendContext};
use clvm_traits::{FromClvm, ToClvm};
use clvm_utils::{CurriedProgram, ToTreeHash, TreeHash};
use clvmr::{Allocator, NodePtr};
use hex_literal::hex;

pub const STREAM_PUZZLE: [u8; 484] =
    hex!("ff02ffff01ff02ffff03ffff09ff81ffffff05ffff14ffff12ff5fffff11ff81bfff2f8080ffff11ff0bff2f80808080ffff01ff04ffff04ff08ffff04ff5fff808080ffff04ffff04ff14ffff04ff81bfff808080ffff04ffff04ff1cffff04ff05ffff04ff81ffffff04ffff04ff05ff8080ff8080808080ffff04ffff04ff1cffff04ffff0bff5effff0bff16ffff0bff16ff6eff1780ffff0bff16ffff0bff7effff0bff16ffff0bff16ff6effff0bffff0101ff178080ffff0bff16ffff0bff7effff0bff16ffff0bff16ff6effff0bffff0101ff81bf8080ffff0bff16ff6eff4e808080ff4e808080ff4e808080ffff04ffff11ff5fff81ff80ffff04ffff04ffff0bffff0173ff0580ff8080ff8080808080ffff04ffff04ff0affff04ffff0117ffff04ff81bfffff04ff05ff8080808080ff808080808080ffff01ff088080ff0180ffff04ffff01ffff49ff5133ff43ff02ffffa04bf5122f344554c53bde2ebb8cd2b7e3d1600ad631c385a5d7cce23c7785459aa09dcf97a184f32623d11a73124ceb99a5709b083721e878a16d78f596718ba7b2ffa102a12871fee210fb8619291eaea194581cbd2531e4b23759d225f6806923f63222a102a8d5dd63fba471ebcb1f3e8f7c1e1879b7152a6e7298a91ce119a63400ade7c5ff018080");
pub const STREAM_PUZZLE_HASH: TreeHash = TreeHash::new(hex!(
    "fb1d2cd040e46ba16f134804d319cfbafb7c736a3c57968e10059b9a5d868773"
));

#[derive(ToClvm, FromClvm, Debug, Clone, Copy, PartialEq, Eq)]
#[clvm(curry)]
pub struct StreamPuzzle1stCurryArgs {
    pub recipient: Bytes32,
    pub end_time: u64,
}

impl StreamPuzzle1stCurryArgs {
    pub fn new(recipient: Bytes32, end_time: u64) -> Self {
        Self {
            recipient,
            end_time,
        }
    }

    pub fn curry_tree_hash(recipient: Bytes32, end_time: u64) -> TreeHash {
        CurriedProgram {
            program: STREAM_PUZZLE_HASH,
            args: StreamPuzzle1stCurryArgs::new(recipient, end_time),
        }
        .tree_hash()
    }
}

#[derive(ToClvm, FromClvm, Debug, Clone, Copy, PartialEq, Eq)]
#[clvm(curry)]
pub struct StreamPuzzle2ndCurryArgs {
    pub self_hash: Bytes32,
    pub last_payment_time: u64,
}

impl StreamPuzzle2ndCurryArgs {
    pub fn new(self_hash: Bytes32, last_payment_time: u64) -> Self {
        Self {
            self_hash,
            last_payment_time,
        }
    }

    pub fn curry_tree_hash(recipient: Bytes32, end_time: u64, last_payment_time: u64) -> TreeHash {
        let self_hash = StreamPuzzle1stCurryArgs::curry_tree_hash(recipient, end_time);
        CurriedProgram {
            program: self_hash,
            args: StreamPuzzle2ndCurryArgs::new(self_hash.into(), last_payment_time),
        }
        .tree_hash()
    }
}

#[derive(ToClvm, FromClvm, Debug, Clone, PartialEq, Eq)]
#[clvm(list)]
pub struct StreamPuzzleSolution {
    pub my_amount: u64,
    pub payment_time: u64,
    #[clvm(rest)]
    pub to_pay: u64,
}

impl Mod for StreamPuzzle1stCurryArgs {
    const MOD_REVEAL: &[u8] = &STREAM_PUZZLE;
    const MOD_HASH: TreeHash = STREAM_PUZZLE_HASH;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamLayer {
    pub recipient: Bytes32,
    pub end_time: u64,
    pub last_payment_time: u64,
}

impl StreamLayer {
    pub fn new(recipient: Bytes32, end_time: u64, last_payment_time: u64) -> Self {
        Self {
            recipient,
            end_time,
            last_payment_time,
        }
    }

    pub fn puzzle_hash(&self) -> TreeHash {
        StreamPuzzle2ndCurryArgs::curry_tree_hash(
            self.recipient,
            self.end_time,
            self.last_payment_time,
        )
    }
}

impl Layer for StreamLayer {
    type Solution = StreamPuzzleSolution;

    fn parse_puzzle(
        allocator: &Allocator,
        puzzle_2nd_curry: Puzzle,
    ) -> Result<Option<Self>, DriverError> {
        let Some(puzzle_2nd_curry) = puzzle_2nd_curry.as_curried() else {
            return Ok(None);
        };

        let Ok(program_2nd_curry) =
            CurriedProgram::<NodePtr, NodePtr>::from_clvm(allocator, puzzle_2nd_curry.curried_ptr)
        else {
            return Ok(None);
        };
        let Some(puzzle_1st_curry) = CurriedPuzzle::parse(allocator, program_2nd_curry.program)
        else {
            return Ok(None);
        };

        let Ok(args1) = StreamPuzzle1stCurryArgs::from_clvm(allocator, puzzle_1st_curry.args)
        else {
            return Ok(None);
        };
        let Ok(args2) = StreamPuzzle2ndCurryArgs::from_clvm(allocator, puzzle_2nd_curry.args)
        else {
            return Ok(None);
        };

        if puzzle_1st_curry.mod_hash != STREAM_PUZZLE_HASH {
            return Err(DriverError::InvalidModHash);
        }

        Ok(Some(Self {
            recipient: args1.recipient,
            end_time: args1.end_time,
            last_payment_time: args2.last_payment_time,
        }))
    }

    fn parse_solution(
        allocator: &Allocator,
        solution: NodePtr,
    ) -> Result<Self::Solution, DriverError> {
        StreamPuzzleSolution::from_clvm(allocator, solution).map_err(DriverError::FromClvm)
    }

    fn construct_puzzle(&self, ctx: &mut SpendContext) -> Result<NodePtr, DriverError> {
        let puzzle_1st_curry =
            ctx.curry(StreamPuzzle1stCurryArgs::new(self.recipient, self.end_time))?;
        let self_hash = StreamPuzzle1stCurryArgs::curry_tree_hash(self.recipient, self.end_time);

        CurriedProgram {
            program: puzzle_1st_curry,
            args: StreamPuzzle2ndCurryArgs::new(self_hash.into(), self.last_payment_time),
        }
        .to_clvm(&mut ctx.allocator)
        .map_err(DriverError::ToClvm)
    }

    fn construct_solution(
        &self,
        ctx: &mut SpendContext,
        solution: Self::Solution,
    ) -> Result<NodePtr, DriverError> {
        StreamPuzzleSolution::to_clvm(&solution, &mut ctx.allocator).map_err(DriverError::ToClvm)
    }
}
