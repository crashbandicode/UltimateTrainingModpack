//! Host value tests use the production token/predicate modules, without native adapters.
extern crate self as training_mod_consts;
pub use training_mod_mii as mii;

#[allow(dead_code)]
#[path = "../../src/source_lifetime.rs"]
mod source_lifetime;
#[allow(dead_code, unexpected_cfgs)]
#[path = "../../src/native_install.rs"]
mod native_install;
#[allow(dead_code, unexpected_cfgs)]
#[path = "../../src/mii_source_provenance.rs"]
mod provenance;

#[test]
fn published_helper_refusal_retains_install_and_loading_source_until_destruction() {
    use native_install::InstallGate;
    use source_lifetime::{NativeEntry, Phase, SourceBook, SourceKey};
    use training_mod_consts::mii::ordinary_player::{Gate, Phase as NativePhase, Ticket};
    let installs = InstallGate::new();
    let install = installs.try_begin(false).unwrap();
    let mut book = SourceBook::<4, 1>::new();
    let load = book
        .begin_install(
            2,
            book.epoch(),
            NativeEntry(0x1200),
            SourceKey {
                kind: 0x48,
                costume: 0,
            },
        )
        .unwrap();
    let ticket = Ticket {
        epoch: 1,
        entry: 0x1200,
        sequence: 1,
    };
    let mut gate = Gate::new(ticket).unwrap();
    assert!(gate.enter(ticket, 6));
    assert!(gate.publish(ticket, 6));
    gate.leave(ticket, 14);
    assert!(gate.can_complete(ticket));
    // An exact completion comparison refused after native parking.
    gate.refuse();
    assert_eq!(gate.phase(), NativePhase::Refused);
    assert!(!gate.can_complete(ticket));
    assert!(!gate.enter(ticket, 14));
    assert_eq!(book.phase(2), Ok(Phase::Loading));
    assert!(book.ready(2).is_err());
    assert!(installs.owns(install));
    assert!(installs.try_begin(false).is_none());
    book.depart_scene().unwrap();
    gate.depart();
    assert!(!gate.can_complete(ticket));
    assert!(installs.owns(install));
    assert_eq!(book.native_destroyed(load.source()), Ok(0));
    assert!(installs.finish(install));
    assert!(!installs.active());
}
