use algorithms::closure_plus;
use arc::Arc;
use fst_traits::{CoreFst, MutableFst};
use semirings::Semiring;
use Result;
use EPS_LABEL;

/// This operation computes the concatenative closure.
/// If A transduces string `x` to `y` with weight `a`,
/// then the closure transduces `x` to `y` with weight `a`,
/// `xx` to `yy` with weight `a ⊗ a`, `xxx` to `yyy` with weight `a ⊗ a ⊗ a`, etc.
/// The empty string is transduced to itself with weight `1` as well.
pub fn closure_star<F>(fst: &mut F) -> Result<()>
where
    F: MutableFst,
{
    closure_plus(fst)?;

    // Add a new start state to allow empty path
    let start_state = fst.start();
    if let Some(start_state_id) = start_state {
        let new_start_state_id = fst.add_state();
        fst.set_start(&new_start_state_id)?;
        fst.add_arc(
            &new_start_state_id,
            Arc::new(
                EPS_LABEL,
                EPS_LABEL,
                <F as CoreFst>::W::one(),
                start_state_id,
            ),
        )?;
    }
    Ok(())
}
