use std::marker::PhantomData;
use std::rc::Rc;

use bimap::Overwritten::Pair;
use failure::_core::cell::RefCell;
use failure::Fallible;

use crate::{Arc, EPS_LABEL, Label, NO_LABEL, NO_STATE_ID};
use crate::algorithms::compose_filters::ComposeFilter;
use crate::algorithms::filter_states::{FilterState, IntegerFilterState, PairFilterState};
use crate::algorithms::lookahead_filters::lookahead_selector::{LookAheadSelector, MatchTypeTrait, selector, selector_2};
use crate::algorithms::lookahead_filters::lookahead_selector::Selector;
use crate::algorithms::lookahead_filters::LookAheadComposeFilterTrait;
use crate::algorithms::lookahead_matchers::LookaheadMatcher;
use crate::algorithms::matchers::{Matcher, MatchType};
use crate::algorithms::matchers::MatcherFlags;
use crate::algorithms::matchers::multi_eps_matcher::MultiEpsMatcher;
use crate::fst_traits::CoreFst;
use crate::semirings::Semiring;

#[derive(Debug, Clone)]
pub struct PushLabelsComposeFilter<
    'fst1,
    'fst2,
    W: Semiring + 'fst1 + 'fst2,
    CF: LookAheadComposeFilterTrait<'fst1, 'fst2, W>,
    SMT: MatchTypeTrait,
> where
    CF::M1: LookaheadMatcher<'fst1, W>,
    CF::M2: LookaheadMatcher<'fst2, W>,
{
    fst1: &'fst1 <CF::M1 as Matcher<'fst1, W>>::F,
    fst2: &'fst2 <CF::M2 as Matcher<'fst2, W>>::F,
    matcher1: Rc<RefCell<MultiEpsMatcher<W, CF::M1>>>,
    matcher2: Rc<RefCell<MultiEpsMatcher<W, CF::M2>>>,
    filter: CF,
    fs: PairFilterState<CF::FS, IntegerFilterState>,
    smt: PhantomData<SMT>,
    narcsa: usize,
}

impl<
        'fst1,
        'fst2,
        W: Semiring + 'fst1 + 'fst2,
        CF: LookAheadComposeFilterTrait<'fst1, 'fst2, W>,
        SMT: MatchTypeTrait,
    > ComposeFilter<'fst1, 'fst2, W> for PushLabelsComposeFilter<'fst1, 'fst2, W, CF, SMT>
where
    CF::M1: LookaheadMatcher<'fst1, W>,
    CF::M2: LookaheadMatcher<'fst2, W>,
{
    type M1 = MultiEpsMatcher<W, CF::M1>;
    type M2 = MultiEpsMatcher<W, CF::M2>;
    type FS = PairFilterState<CF::FS, IntegerFilterState>;

    fn new<IM1: Into<Option<Rc<RefCell<Self::M1>>>>, IM2: Into<Option<Rc<RefCell<Self::M2>>>>>(
        fst1: &'fst1 <Self::M1 as Matcher<'fst1, W>>::F,
        fst2: &'fst2 <Self::M2 as Matcher<'fst2, W>>::F,
        m1: IM1,
        m2: IM2,
    ) -> Fallible<Self> {
        let filter = CF::new(
            fst1,
            fst2,
            m1.into().map(|e| e.borrow().matcher()),
            m2.into().map(|e| e.borrow().matcher()),
        )?;
        unimplemented!()
    }

    fn start(&self) -> Self::FS {
        PairFilterState::new((self.filter.start(), FilterState::new(NO_LABEL)))
    }

    fn set_state(&mut self, s1: usize, s2: usize, filter_state: &Self::FS) {
        self.fs = filter_state.clone();
        self.filter.set_state(s1, s2, filter_state.state1());
        if !self
            .filter
            .lookahead_flags()
            .contains(MatcherFlags::LOOKAHEAD_PREFIX)
        {
            return;
        }
        self.narcsa = if self.filter.lookahead_output() {
            self.fst1.num_arcs(s1).unwrap()
        } else {
            self.fst2.num_arcs(s2).unwrap()
        };
        let fs2 = filter_state.state2();
        let flabel = fs2.state();
        self.matcher1().borrow_mut().clear_multi_eps_labels();
        self.matcher2().borrow_mut().clear_multi_eps_labels();
        if *flabel != NO_LABEL {
            self.matcher1().borrow_mut().add_multi_eps_label(*flabel);
            self.matcher2().borrow_mut().add_multi_eps_label(*flabel);
        }
    }

    fn filter_arc(&mut self, arc1: &mut Arc<W>, arc2: &mut Arc<W>) -> Self::FS {
        if !self
            .filter
            .lookahead_flags()
            .contains(MatcherFlags::LOOKAHEAD_PREFIX)
        {
            return FilterState::new((
                self.filter.filter_arc(arc1, arc2),
                FilterState::new(NO_LABEL),
            ));
        }
        let fs2 = self.fs.state2();
        let flabel = fs2.state();
        if *flabel != NO_LABEL {
            if self.filter.lookahead_output() {
                return self.pushed_label_filter_arc(arc1, arc2, *flabel);
            } else {
                return self.pushed_label_filter_arc(arc2, arc1, *flabel);
            }
        }
        let fs1 = self.filter.filter_arc(arc1, arc2);
        if fs1 == FilterState::new_no_state() {
            return FilterState::new_no_state();
        }
        if !self.filter.lookahead_arc() {
            return FilterState::new((fs1, FilterState::new(NO_LABEL)));
        }
        if self.filter.lookahead_output() {
            self.push_label_filter_arc(arc1, arc2, &fs1)
        } else {
            self.push_label_filter_arc(arc2, arc1, &fs1)
        }
    }

    fn filter_final(&self, w1: &mut W, w2: &mut W) {
        self.filter.filter_final(w1, w2);
        if !self
            .filter
            .lookahead_flags()
            .contains(MatcherFlags::LOOKAHEAD_PREFIX)
            || w1.is_zero()
        {
            return;
        }
        let fs2 = self.fs.state2();
        let flabel = fs2.state();
        if *flabel != NO_LABEL {
            *w1 = W::zero()
        }
    }

    fn matcher1(&self) -> Rc<RefCell<Self::M1>> {
        Rc::clone(&self.matcher1)
    }

    fn matcher2(&self) -> Rc<RefCell<Self::M2>> {
        Rc::clone(&self.matcher2)
    }
}

impl<
    'fst1,
    'fst2,
    W: Semiring + 'fst1 + 'fst2,
    CF: LookAheadComposeFilterTrait<'fst1, 'fst2, W>,
    SMT: MatchTypeTrait,
> PushLabelsComposeFilter<'fst1, 'fst2, W, CF, SMT>
    where
        CF::M1: LookaheadMatcher<'fst1, W>,
        CF::M2: LookaheadMatcher<'fst2, W>,
{
    // Consumes an already pushed label.
    fn pushed_label_filter_arc(&self, arca: &mut Arc<W>, arcb: &mut Arc<W>, flabel: Label) -> <Self as ComposeFilter<'fst1, 'fst2, W>>::FS {
        let labela = if self.filter.lookahead_output() {
            &mut arca.olabel
        } else {
            &mut arca.ilabel
        };
        let labelb = if self.filter.lookahead_output() {
            arcb.ilabel
        } else {
            arcb.olabel
        };

        if labelb != NO_LABEL {
            FilterState::new_no_state()
        } else if *labela == flabel {
            *labela = EPS_LABEL;
            self.start()
        } else if *labela == EPS_LABEL {
            if self.narcsa == 1 {
                self.fs.clone()
            } else {
                let fn1 = |selector: LookAheadSelector<<CF::M1 as Matcher<'fst1, W>>::F, CF::M2>| {
                    if selector.matcher.borrow_mut().lookahead_label(arca.nextstate, flabel).unwrap() {
                        self.fs.clone()
                    } else {
                        FilterState::new_no_state()
                    }
                };

                let fn2 = |selector: LookAheadSelector<<CF::M2 as Matcher<'fst2, W>>::F, CF::M1>| {
                    if selector.matcher.borrow_mut().lookahead_label(arca.nextstate, flabel).unwrap() {
                        self.fs.clone()
                    } else {
                        FilterState::new_no_state()
                    }
                };
                selector(
                    self.filter.matcher1(),
                    self.filter.matcher2(),
                    SMT::match_type(),
                    self.filter.lookahead_type(),
                    fn1,
                    fn2,
                )
            }
        } else {
            FilterState::new_no_state()
        }
    }

    // Pushes a label forward when possible.
    fn push_label_filter_arc(&self, arca: &mut Arc<W>, arcb: &mut Arc<W>, fs1: &CF::FS) -> <Self as ComposeFilter<'fst1, 'fst2, W>>::FS {
        let labela = if self.filter.lookahead_output() {
            &mut arca.olabel
        } else {
            &mut arca.ilabel
        };
        let labelb = if self.filter.lookahead_output() {
            arcb.olabel
        } else {
            arcb.ilabel
        };

        if labelb != EPS_LABEL {
            return FilterState::new((fs1.clone(), FilterState::new(NO_LABEL)))
        }

        if *labela != EPS_LABEL && self.filter.lookahead_flags().contains(MatcherFlags::LOOKAHEAD_NON_EPSILON_PREFIX) {
            return FilterState::new((fs1.clone(), FilterState::new(NO_LABEL)))
        }

        let mut larc = Arc::new(NO_LABEL, NO_LABEL, W::zero(), NO_STATE_ID);

        let b = match selector_2(
            self.filter.matcher1(),
            self.filter.matcher2(),
            SMT::match_type(),
            self.filter.lookahead_type(),
        ) {
            Selector::MatchInput(s) => s.matcher.borrow().lookahead_prefix(&mut larc),
            Selector::MatchOutput(s) => s.matcher.borrow().lookahead_prefix(&mut larc)
        };

        if b {
            *labela = if self.filter.lookahead_output() {
                larc.ilabel
            } else {
                larc.olabel
            };
            arcb.ilabel = larc.ilabel;
            arcb.olabel = larc.olabel;
            arcb.weight.times_assign(&larc.weight).unwrap();
            arcb.nextstate = larc.nextstate;
            FilterState::new((fs1.clone(), FilterState::new(*labela)))
        } else {
            FilterState::new((fs1.clone(), FilterState::new(NO_LABEL)))
        }
    }
}

// impl<
//         'fst1,
//         'fst2,
//         W: Semiring + 'fst1 + 'fst2,
//         CF: LookAheadComposeFilterTrait<'fst1, 'fst2, W>,
//         SMT: MatchTypeTrait,
//     > LookAheadComposeFilterTrait<'fst1, 'fst2, W>
//     for PushLabelsComposeFilter<'fst1, 'fst2, W, CF, SMT>
// where
//     CF::M1: LookaheadMatcher<'fst1, W>,
//     CF::M2: LookaheadMatcher<'fst2, W>,
// {
//     fn lookahead_flags(&self) -> MatcherFlags {
//         unimplemented!()
//     }
//
//     fn lookahead_arc(&self) -> bool {
//         unimplemented!()
//     }
//
//     fn lookahead_type(&self) -> MatchType {
//         unimplemented!()
//     }
// }
