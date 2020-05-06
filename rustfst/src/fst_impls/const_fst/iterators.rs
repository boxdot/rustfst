use std::iter::Enumerate;
use std::iter::Map;
use std::iter::Skip;
use std::iter::Take;
use std::iter::Zip;
use std::ops::Range;
use std::slice;

use anyhow::Result;
use itertools::Itertools;
use itertools::{izip, repeat_n, RepeatN};

use crate::fst_impls::const_fst::data_structure::ConstState;
use crate::fst_impls::ConstFst;
use crate::fst_traits::FstIterData;
use crate::fst_traits::{FstIntoIterator, FstIterator, StateIterator};
use crate::semirings::Semiring;
use crate::{StateId, TrsConst};
use crate::Tr;
use std::sync::Arc;

impl<W> ConstFst<W> {
    fn state_range(&self) -> Range<usize> {
        0..self.states.len()
    }

    fn tr_range(&self, state: &ConstState<W>) -> Range<usize> {
        state.pos..state.pos + state.ntrs
    }
}

// impl<W: Semiring> FstIntoIterator for ConstFst<W>
// where
//     W: 'static,
// {
//     type TrsIter = std::vec::IntoIter<Tr<W>>;
//
//     // TODO: Change this to impl once the feature has been stabilized
//     // #![feature(type_alias_impl_trait)]
//     // https://github.com/rust-lang/rust/issues/63063)
//     type FstIter = Box<dyn Iterator<Item = FstIterData<W, Self::TrsIter>>>;
//
//     fn fst_into_iter(mut self) -> Self::FstIter {
//         // Here the contiguous trs are moved into multiple vectors in order to be able to create
//         // iterator for each states.
//         // TODO: Find a way to avoid this allocation.
//         let mut trs = Vec::with_capacity(self.states.len());
//         for const_state in &self.states {
//             trs.push(self.trs.drain(0..const_state.ntrs).collect_vec())
//         }
//
//         Box::new(
//             izip!(self.states.into_iter(), trs.into_iter())
//                 .enumerate()
//                 .map(|(state_id, (const_state, trs_from_state))| FstIterData {
//                     state_id,
//                     trs: trs_from_state.into_iter(),
//                     final_weight: const_state.final_weight,
//                     num_trs: const_state.ntrs,
//                 }),
//         )
//     }
// }

impl<'a, W> StateIterator<'a> for ConstFst<W> {
    type Iter = Range<StateId>;
    fn states_iter(&'a self) -> Self::Iter {
        self.state_range()
    }
}

impl<'a, W: Semiring + 'static> FstIterator<'a> for ConstFst<W> {
    type FstIter = Map<
        Enumerate<Zip<std::slice::Iter<'a, ConstState<W>>, RepeatN<&'a Arc<Vec<Tr<W>>>>>>,
        Box<
            dyn FnMut(
                (StateId, (&'a ConstState<W>, &'a Arc<Vec<Tr<W>>>)),
            ) -> FstIterData<&'a W, Self::TRS>,
        >,
    >;
    fn fst_iter(&'a self) -> Self::FstIter {
        let it = repeat_n(&self.trs, self.states.len());
        izip!(self.states.iter(), it).enumerate().map(Box::new(
            |(state_id, (fst_state, trs)): (StateId, (&'a ConstState<W>, &'a Arc<Vec<Tr<W>>>))| {
                FstIterData {
                    state_id,
                    trs: TrsConst{
                        trs: Arc::clone(trs),
                        pos: fst_state.pos,
                        n: fst_state.ntrs
                    },
                    final_weight: fst_state.final_weight.as_ref(),
                    num_trs: fst_state.ntrs,
                }
            },
        ))
    }
}
