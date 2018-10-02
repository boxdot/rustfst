use fst_traits::{ExpandedFst, Fst, MutableFst};
use std::collections::HashSet;
use Result;
use StateId;

fn dfs<F: Fst>(
    fst: &F,
    state_id_cour: StateId,
    accessible_states: &mut HashSet<StateId>,
    coaccessible_states: &mut HashSet<StateId>,
) -> Result<()> {
    accessible_states.insert(state_id_cour);
    let mut is_coaccessible = fst.is_final(&state_id_cour);
    for arc in fst.arcs_iter(&state_id_cour)? {
        let nextstate = arc.nextstate;

        if !accessible_states.contains(&nextstate) {
            dfs(fst, nextstate, accessible_states, coaccessible_states)?;
        }

        if coaccessible_states.contains(&nextstate) {
            is_coaccessible = true;
        }
    }

    if is_coaccessible {
        coaccessible_states.insert(state_id_cour);
    }

    Ok(())
}

pub fn connect<F: ExpandedFst + MutableFst>(fst: &mut F) -> Result<()> {
    let mut accessible_states = HashSet::new();
    let mut coaccessible_states = HashSet::new();

    if let Some(state_id) = fst.start() {
        dfs(
            fst,
            state_id,
            &mut accessible_states,
            &mut coaccessible_states,
        )?;
    }

    let mut to_delete = Vec::new();
    for i in 0..fst.num_states() {
        if !accessible_states.contains(&i) || !coaccessible_states.contains(&i) {
            to_delete.push(i);
        }
    }
    fst.del_states(to_delete)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use arc::Arc;
    use fst_impls::VectorFst;
    use semirings::ProbabilityWeight;

    #[test]
    fn test_connect() {
        let mut fst = VectorFst::new();
        let s1 = fst.add_state();
        let s2 = fst.add_state();
        fst.set_start(&s1).unwrap();
        fst.add_arc(&s1, Arc::new(3, 5, ProbabilityWeight::new(10.0), s2))
            .unwrap();
        fst.add_arc(&s1, Arc::new(5, 7, ProbabilityWeight::new(18.0), s2))
            .unwrap();
        fst.set_final(&s2, ProbabilityWeight::new(31.0)).unwrap();
        fst.add_state();
        let s4 = fst.add_state();
        fst.add_arc(&s2, Arc::new(5, 7, ProbabilityWeight::new(18.0), s4))
            .unwrap();
        assert_eq!(fst.num_states(), 4);
        connect(&mut fst).unwrap();
        assert_eq!(fst.num_states(), 2);
    }
}
