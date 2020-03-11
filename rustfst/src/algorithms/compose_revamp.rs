use std::cell::RefCell;
use std::hash::Hash;
use std::rc::Rc;

use failure::Fallible;

use crate::algorithms::cache::{CacheImpl, FstImpl, StateTable};
// use crate::algorithms::compose_filters::{
//     AltSequenceComposeFilter, ComposeFilter, MatchComposeFilter, NoMatchComposeFilter,
//     NullComposeFilter, SequenceComposeFilter, TrivialComposeFilter,
// };
use crate::algorithms::dynamic_fst::DynamicFst;
use crate::algorithms::matchers::{MatchType, SortedMatcher};
use crate::algorithms::matchers::{Matcher, MatcherFlags};
use crate::fst_traits::{CoreFst, Fst, MutableFst};
use crate::semirings::Semiring;
use crate::{Arc, StateId, EPS_LABEL, NO_LABEL};
use crate::algorithms::compose_filters::{TrivialComposeFilter, ComposeFilter};

#[derive(Default)]
pub struct ComposeFstOptions<M, CF, ST> {
    matcher1: Option<M>,
    matcher2: Option<M>,
    filter: Option<CF>,
    state_table: Option<ST>,
}

impl<M, CF, ST> ComposeFstOptions<M, CF, ST> {
    pub fn new<
        IM1: Into<Option<M>>,
        IM2: Into<Option<M>>,
        ICF: Into<Option<CF>>,
        IST: Into<Option<ST>>,
    >(
        matcher1: IM1,
        matcher2: IM2,
        filter: ICF,
        state_table: IST,
    ) -> Self {
        Self {
            matcher1: matcher1.into(),
            matcher2: matcher2.into(),
            filter: filter.into(),
            state_table: state_table.into(),
        }
    }
}

pub struct ComposeFstImplOptions<M1, M2, CF, ST> {
    matcher1: Option<M1>,
    matcher2: Option<M2>,
    filter: Option<CF>,
    state_table: Option<ST>,
    allow_noncommute: bool,
}

impl<M1, M2, CF, ST> Default for ComposeFstImplOptions<M1, M2, CF, ST> {
    fn default() -> Self {
        Self {
            matcher1: None,
            matcher2: None,
            filter: None,
            state_table: None,
            allow_noncommute: false
        }
    }
}

impl<M1, M2, CF, ST> ComposeFstImplOptions<M1, M2, CF, ST> {
    pub fn new<
        IM1: Into<Option<M1>>,
        IM2: Into<Option<M2>>,
        ICF: Into<Option<CF>>,
        IST: Into<Option<ST>>,
    >(
        matcher1: IM1,
        matcher2: IM2,
        filter: ICF,
        state_table: IST,
        allow_noncommute: bool,
    ) -> Self {
        Self {
            matcher1: matcher1.into(),
            matcher2: matcher2.into(),
            filter: filter.into(),
            state_table: state_table.into(),
            allow_noncommute,
        }
    }
}

#[derive(Default, PartialEq, Eq, Clone, Hash, PartialOrd, Debug)]
struct ComposeStateTuple<FS> {
    fs: FS,
    s1: StateId,
    s2: StateId,
}

#[derive(Clone, Debug)]
pub struct ComposeFstImpl<
    'fst,
    W: Semiring + 'fst,
    CF: ComposeFilter<'fst, W>,
> {
    fst1: &'fst <CF::M1 as Matcher<'fst, W>>::F,
    fst2: &'fst <CF::M2 as Matcher<'fst, W>>::F,
    matcher1: Rc<RefCell<CF::M1>>,
    matcher2: Rc<RefCell<CF::M2>>,
    compose_filter: CF,
    cache_impl: CacheImpl<W>,
    state_table: StateTable<ComposeStateTuple<CF::FS>>,
    match_type: MatchType,
}

impl<'fst, W: Semiring + 'fst, CF: ComposeFilter<'fst, W>>
    ComposeFstImpl<'fst, W, CF>
{
    // Compose specifying two matcher types Matcher1 and Matcher2. Requires input
    // FST (of the same Arc type, but o.w. arbitrary) match the corresponding
    // matcher FST types). Recommended only for advanced use in demanding or
    // specialized applications due to potential code bloat and matcher
    // incompatibilities.
    // fn new2(fst1: &'fst F1, fst2: &'fst F2) -> Fallible<Self> {
    //     unimplemented!()
    // }

    fn new(
        fst1: &'fst <CF::M1 as Matcher<'fst, W>>::F,
        fst2: &'fst <CF::M2 as Matcher<'fst, W>>::F,
        opts: ComposeFstImplOptions<CF::M1, CF::M2, CF, StateTable<ComposeStateTuple<CF::FS>>>,
    ) -> Fallible<Self> {
        let opts_matcher1 = opts.matcher1;
        let opts_matcher2 = opts.matcher2;
        let compose_filter = opts
            .filter
            .unwrap_or_else(|| CF::new(fst1, fst2, opts_matcher1, opts_matcher2).unwrap());
        let matcher1 = compose_filter.matcher1();
        let matcher2 = compose_filter.matcher2();
        Ok(Self {
            fst1,
            fst2,
            compose_filter,
            cache_impl: CacheImpl::new(),
            state_table: opts.state_table.unwrap_or_else(StateTable::new),
            match_type: Self::match_type(&matcher1, &matcher2)?,
            matcher1,
            matcher2,
        })
    }

    fn match_type(
        matcher1: &Rc<RefCell<CF::M1>>,
        matcher2: &Rc<RefCell<CF::M2>>,
    ) -> Fallible<MatchType> {
        if matcher1
            .borrow()
            .flags()
            .contains(MatcherFlags::REQUIRE_MATCH)
            && matcher1.borrow().match_type() == MatchType::MatchOutput
        {
            bail!("ComposeFst: 1st argument cannot perform required matching (sort?)")
        }
        if matcher2
            .borrow()
            .flags()
            .contains(MatcherFlags::REQUIRE_MATCH)
            && matcher2.borrow().match_type() == MatchType::MatchInput
        {
            bail!("ComposeFst: 2nd argument cannot perform required matching (sort?)")
        }

        let type1 = matcher1.borrow().match_type();
        let type2 = matcher2.borrow().match_type();
        let mt = if type1 == MatchType::MatchOutput && type2 == MatchType::MatchInput {
            MatchType::MatchBoth
        } else if type1 == MatchType::MatchOutput {
            MatchType::MatchOutput
        } else if type2 == MatchType::MatchInput {
            MatchType::MatchInput
        } else {
            bail!("ComposeFst: 1st argument cannot match on output labels and 2nd argument cannot match on input labels (sort?).")
        };
        Ok(mt)
    }

    fn match_input(&self, _s1: StateId, _s2: StateId) -> bool {
        match self.match_type {
            MatchType::MatchInput => true,
            MatchType::MatchOutput => false,
            _ => unimplemented!(),
        }
    }

    fn ordered_expand<'b,FA: Fst<W = W> + 'b,  FB: Fst<W = W> + 'b, M: Matcher<'b, W, F=FA>>(
        &mut self,
        s: StateId,
        sa: StateId,
        fstb: &FB,
        sb: StateId,
        matchera: Rc<RefCell<M>>,
        match_input: bool,
    ) -> Fallible<()> where W: 'b{
        let arc_loop = if match_input {
            Arc::new(EPS_LABEL, NO_LABEL, W::one(), sb)
        } else {
            Arc::new(NO_LABEL, EPS_LABEL, W::one(), sb)
        };
        self.match_arc(s, sa, Rc::clone(&matchera), &arc_loop, match_input)?;
        for arc in fstb.arcs_iter(sb)? {
            self.match_arc(s, sa, Rc::clone(&matchera), arc, match_input)?;
        }
        Ok(())
    }

    fn add_arc(
        &mut self,
        s: StateId,
        mut arc1: Arc<W>,
        arc2: Arc<W>,
        fs: CF::FS,
    ) -> Fallible<()> {
        let tuple = ComposeStateTuple {
            fs,
            s1: arc1.nextstate,
            s2: arc2.nextstate,
        };
        arc1.weight.times_assign(arc2.weight)?;
        self.cache_impl.push_arc(
            s,
            Arc::new(
                arc1.ilabel,
                arc2.olabel,
                arc1.weight,
                self.state_table.find_id(tuple),
            ),
        )?;

        Ok(())
    }

    fn match_arc<'b, M: Matcher<'b, W>>(
        &mut self,
        s: StateId,
        sa: StateId,
        matchera: Rc<RefCell<M>>,
        arc: &Arc<W>,
        match_input: bool,
    ) -> Fallible<()> where W: 'b{
        let label = if match_input { arc.olabel } else { arc.ilabel };

        for arca in matchera.borrow().iter(sa, label)? {
            let mut arca = arca.into_arc(
                sa,
                if match_input {
                    MatchType::MatchInput
                } else {
                    MatchType::MatchOutput
                },
            )?;
            let mut arcb = arc.clone();
            if match_input {
                let opt_fs = self.compose_filter.filter_arc(&mut arcb, &mut arca);
                if let Some(fs) = opt_fs {
                    self.add_arc(s, arcb, arca, fs)?;
                }
            } else {
                let opt_fs = self.compose_filter.filter_arc(&mut arca, &mut arcb);
                if let Some(fs) = opt_fs {
                    self.add_arc(s, arca, arcb, fs)?;
                }
            }
        }

        Ok(())
    }
}

impl<'fst, W: Semiring + 'fst + 'static, CF: ComposeFilter<'fst, W>> FstImpl
    for ComposeFstImpl<'fst, W, CF>
{
    type W = W;

    fn cache_impl_mut(&mut self) -> &mut CacheImpl<Self::W> {
        &mut self.cache_impl
    }

    fn cache_impl_ref(&self) -> &CacheImpl<Self::W> {
        &self.cache_impl
    }

    fn expand(&mut self, state: usize) -> Fallible<()> {
        let tuple = self.state_table.find_tuple(state);
        let s1 = tuple.s1;
        let s2 = tuple.s2;
        self.compose_filter.set_state(s1, s2, &tuple.fs);
        drop(tuple);
        if self.match_input(s1, s2) {
            self.ordered_expand(state, s2, self.fst1, s1, Rc::clone(&self.matcher2), true)?;
        } else {
            self.ordered_expand(state, s1, self.fst2, s2, Rc::clone(&self.matcher1), false)?;
        }
        Ok(())
    }

    fn compute_start(&mut self) -> Fallible<Option<StateId>> {
        let s1 = self.fst1.start();
        if s1.is_none() {
            return Ok(None);
        }
        let s1 = s1.unwrap();
        let s2 = self.fst2.start();
        if s2.is_none() {
            return Ok(None);
        }
        let s2 = s2.unwrap();
        let fs = self.compose_filter.start();
        let tuple = ComposeStateTuple { s1, s2, fs };
        Ok(Some(self.state_table.find_id(tuple)))
    }

    fn compute_final(&mut self, state: usize) -> Fallible<Option<Self::W>> {
        let tuple = self.state_table.find_tuple(state);

        let s1 = tuple.s1;
        let final1 = self.compose_filter.matcher1().borrow().final_weight(s1)?;
        if final1.is_none() {
            return Ok(None);
        }
        let mut final1 = final1.unwrap().clone();

        let s2 = tuple.s2;
        let final2 = self.compose_filter.matcher2().borrow().final_weight(s2)?;
        if final2.is_none() {
            return Ok(None);
        }
        let mut final2 = final2.unwrap().clone();

        self.compose_filter.set_state(s1, s2, &tuple.fs);
        self.compose_filter.filter_final(&mut final1, &mut final2);

        final1.times_assign(&final2)?;
        Ok(Some(final1))
    }
}

#[derive(PartialOrd, PartialEq, Debug, Clone, Copy)]
pub enum ComposeFilterEnum {
    AutoFilter,
    NullFilter,
    TrivialFilter,
    SequenceFilter,
    AltSequenceFilter,
    MatchFilter,
    NoMatchFilter,
}

pub type ComposeFst<'fst, W, CF> = DynamicFst<ComposeFstImpl<'fst, W, CF>>;


fn create_base_2<
    'fst,
    W: Semiring + 'fst,
    CF: ComposeFilter<'fst, W>,
>(
    fst1: &'fst <CF::M1 as Matcher<'fst, W>>::F,
    fst2: &'fst <CF::M2 as Matcher<'fst, W>>::F,
    opts: ComposeFstImplOptions<CF::M1, CF::M2, CF, StateTable<ComposeStateTuple<CF::FS>>>,
) -> Fallible<ComposeFstImpl<'fst, W, CF>>
{
    ComposeFstImpl::new(fst1, fst2, opts)
}

// fn create_base_1<
//     'fst,
//     F1: Fst + 'fst,
//     F2: Fst<W = F1::W> + 'fst,
//     CF: ComposeFilter<'fst, F1, F2>,
// >(
//     fst1: &'fst F1,
//     fst2: &'fst F2,
//     opts: ComposeFstImplOptions<CF::M1, CF::M2, CF, StateTable<ComposeStateTuple<CF::FS>>>,
// ) -> Fallible<ComposeFstImpl<'fst, F1, F2, CF>>
//     where F1::W: 'static
// {
//     ComposeFstImpl::new(fst1, fst2, opts)
// }

impl<'fst, W: Semiring + 'fst, CF: ComposeFilter<'fst, W>>
    ComposeFst<'fst, W, CF>
{
    // TODO: Change API, no really user friendly
    pub fn new(fst1: &'fst <CF::M1 as Matcher<'fst, W>>::F, fst2: &'fst <CF::M2 as Matcher<'fst, W>>::F) -> Fallible<Self> where W: 'static {
        let isymt = fst1.input_symbols();
        let osymt = fst2.output_symbols();
        let compose_impl = create_base_2(fst1, fst2, ComposeFstImplOptions::default())?;
        Ok(Self::from_impl(compose_impl, isymt, osymt))
    }

    // pub fn new_with_options(fst1: &'fst F1, fst2: &'fst F2, opts: ComposeFstOptions) -> Fallible<Self> {
    //     unimplemented!()
    // }
    //
    // pub fn new_with_impl_optins(fst1: &'fst F1, fst2: &'fst F2: opts: ComposeFstImplOptions) -> Fallible<Self> {
    //     unimplemented!()
    // }
}

#[derive(PartialOrd, PartialEq, Debug, Clone, Copy)]
pub struct ComposeConfig {
    compose_filter: ComposeFilterEnum,
    connect: bool,
}

impl Default for ComposeConfig {
    fn default() -> Self {
        Self {
            compose_filter: ComposeFilterEnum::AutoFilter,
            connect: true,
        }
    }
}

pub fn compose_with_config<F1: Fst, F2: Fst<W = F1::W>, F3: MutableFst<W = F1::W>>(
    fst1: &F1,
    fst2: &F2,
    config: ComposeConfig,
) -> Fallible<F3>
where
    F1::W: 'static,
{
    let mut ofst: F3 = match config.compose_filter {
        ComposeFilterEnum::AutoFilter => {
            // ComposeFst::<_, _, SequenceComposeFilter<_, SortedMatcher<_>, SortedMatcher<_>>>::new(
            //     fst1, fst2,
            // )?
            // .compute()?
            unimplemented!()
        }
        ComposeFilterEnum::NullFilter => {
            // ComposeFst::<_, _, NullComposeFilter<SortedMatcher<_>, SortedMatcher<_>>>::new(
            //     fst1, fst2,
            // )?
            // .compute()?
            unimplemented!()
        }
        ComposeFilterEnum::SequenceFilter => {
            // ComposeFst::<_, _, SequenceComposeFilter<_, SortedMatcher<_>, SortedMatcher<_>>>::new(
            //     fst1, fst2,
            // )?
            // .compute()?
            unimplemented!()
        }
        ComposeFilterEnum::AltSequenceFilter =>
        //     ComposeFst::<
        //     _,
        //     _,
        //     AltSequenceComposeFilter<_, SortedMatcher<_>, SortedMatcher<_>>,
        // >::new(fst1, fst2)?
        // .compute()?,
        unimplemented!(),
        ComposeFilterEnum::MatchFilter => {
            // ComposeFst::<_, _, MatchComposeFilter<_, _, SortedMatcher<_>, SortedMatcher<_>>>::new(
            //     fst1, fst2,
            // )?
            // .compute()?
            unimplemented!()
        }
        ComposeFilterEnum::NoMatchFilter => {
            // ComposeFst::<_, _, NoMatchComposeFilter<SortedMatcher<_>, SortedMatcher<_>>>::new(
            //     fst1, fst2,
            // )?
            // .compute()?
            unimplemented!()
        }
        ComposeFilterEnum::TrivialFilter => {
            ComposeFst::<_, TrivialComposeFilter<SortedMatcher<_>, SortedMatcher<_>>>::new(
                fst1, fst2,
            )?
            .compute()?
        }
    };

    if config.connect {
        crate::algorithms::connect(&mut ofst)?;
    }

    Ok(ofst)
}

pub fn compose<F1: Fst, F2: Fst<W = F1::W>, F3: MutableFst<W = F1::W>>(
    fst1: &F1,
    fst2: &F2,
) -> Fallible<F3>
where
    F1::W: 'static,
{
    let config = ComposeConfig::default();
    compose_with_config(fst1, fst2, config)
}
