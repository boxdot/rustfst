use crate::fst_traits::{Fst, StateIterator};
use crate::semirings::Semiring;
use crate::StateId;
use std::marker::PhantomData;

#[derive(Debug)]
pub struct FinalState<'f, W> {
    pub state_id: StateId,
    pub final_weight: &'f W,
}

/// Trait to iterate over the final states of a wFST.
pub trait FinalStatesIterator<'f, W>
where W: Semiring + 'f
{
    type Iter: Iterator<Item = FinalState<'f, W>>;
    fn final_states_iter(&'f self) -> Self::Iter;
}

impl<'f, W, F> FinalStatesIterator<'f, W> for F
where
    W: Semiring + 'f,
    F: 'f + Fst<W>,
{
    type Iter = StructFinalStatesIterator<'f, W, F>;
    fn final_states_iter(&'f self) -> Self::Iter {
        StructFinalStatesIterator::new(&self)
    }
}

pub struct StructFinalStatesIterator<'f, W, F>
where
    W: Semiring + 'f,
    F: 'f + Fst<W>,
{
    fst: &'f F,
    it: <F as StateIterator<'f>>::Iter,
    w: PhantomData<W>
}

impl<'f, W, F> StructFinalStatesIterator<'f, W, F>
where
    W: Semiring + 'f,
    F: 'f + Fst<W>,
{
    fn new(fst: &'f F) -> StructFinalStatesIterator<W, F> {
        StructFinalStatesIterator {
            fst,
            it: fst.states_iter(),
            w: PhantomData
        }
    }
}

impl<'f, W, F> Iterator for StructFinalStatesIterator<'f, W, F>
where
    W: Semiring + 'f,
    F: 'f + Fst<W>,
{
    type Item = FinalState<'f, W>;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(state_id) = self.it.next() {
            if let Some(final_weight) = unsafe { self.fst.final_weight_unchecked(state_id) } {
                return Some(FinalState {
                    state_id,
                    final_weight: &final_weight,
                });
            }
        }
        None
    }
}
