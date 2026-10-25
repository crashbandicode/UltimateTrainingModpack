//! Scene policy and fresh native readiness for the three exact Mii backends:
//! Training, local-wireless waiting and Quickplay waiting (the two waiting
//! backends share one PracticeScene shape and one live-shape gate).
pub(super) use training_mod_consts::mii::backend::Layout;
pub(super) fn current()->Option<Layout> {
    let mode=crate::common::game_mode::current()?;
    Layout::scene(mode.initialized,mode.melee,mode.mode,
        cfg!(feature="waiting-roster-reload") && super::lobby::active())
}
pub(super) fn cpu()->Option<u32> {Some(current()?.visible()[1])}
pub(super) unsafe fn ready()->bool {
    let Some(layout)=current() else{return false;};
    if !layout.quantity(super::native_source::training_cpu_quantity()) {return false;}
    if layout==Layout::Training {return true;}
    let Some(helper)=super::native_source::entry(5) else{return false;};
    super::native_source::exact_waiting_layout(helper,5)
        && layout.visible().iter().copied().all(|id|super::native_source::entry(id).is_some_and(|entry|
            super::ready_single_mii(entry,id).is_some() || super::ready_forms(entry,id).is_some()))
}
