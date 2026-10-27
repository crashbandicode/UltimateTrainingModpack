use training_mod_mii::inventory::{build_snapshot, Canonical, Finding, MiiClass, RawSlot, Snapshot, TaggedId};
use training_mod_mii::selection::{Action, Active, Origin, PlayerLive, PlayerOrigin, Preferences, Role};
use training_mod_tui::mii_menu::{CatalogMenu, Context, MenuCapture};
use training_mod_tui::{App, AppPage, InputControl, StatefulTable, SubMenu, SubMenuType, Tab};

fn slot(id: u32, class: MiiClass, face: u8) -> RawSlot {
    let mut charinfo = [face; 0x58];
    charinfo[..0x26].fill(0);
    RawSlot { index: id as usize, slot_id: id, name_len: 1,
        name_units: [u16::from(b'A'), 0, 0, 0, 0, 0, 0, 0, 0, 0],
        colour: 0, voice: 37, specials: [0; 4],
        tag_outfit: TaggedId(0x1300291bfef7a5f7), tag_hat: TaggedId(0x5a00000fa9efa5ea),
        tag_chara: TaggedId(0xc100000000000000 | class.chara_hash40()), charinfo }
}

fn catalog() -> Snapshot {
    build_snapshot(vec![
        slot(2622, MiiClass::Swordfighter, 1), slot(2700, MiiClass::Swordfighter, 2),
        slot(2800, MiiClass::Swordfighter, 2), // a copy must not become another dropdown row
        slot(10, MiiClass::Brawler, 3), slot(20, MiiClass::Gunner, 4),
    ])
}

fn canonical(snapshot: &Snapshot, id: u32) -> Canonical {
    snapshot.configs.iter().find(|config| config.slot_id == id).unwrap().canonical
}

fn app() -> App<'static> {
    let mut app = App::new();
    app.tabs.push(Tab { id: "character_extras", title: "Character Extras",
        submenus: StatefulTable::with_items(8, 4, vec![SubMenu {
            title: "Solo Popo", id: "solo_popo_original", help_text: "",
            submenu_type: SubMenuType::ToggleSingle, toggles: StatefulTable::new(1, 1), slider: None,
        }]) });
    app.tabs.state.select(Some(0));
    app.serialized_default_settings = "{}".into();
    app
}

#[test]
fn preset_failure_keeps_native_choices_usable_but_native_defects_still_block() {
    let mut snapshot = catalog();
    snapshot.findings.push(Finding::PresetSource { error: "invalid preset file".into() });
    let wanted = canonical(&snapshot, 20);
    let mut preferences = Preferences::default();
    let context = Context { epoch: 1, original: PlayerOrigin::NotMii,
        active: Active { player: PlayerLive::NotMii, cpu: None }, cpu_original: None };
    assert!(snapshot.is_complete());
    let choice = training_mod_mii::selection::resolve(&snapshot, &preferences,
        Role::Cpu, MiiClass::Gunner, context.original).unwrap();
    assert!(snapshot.configuration(choice).is_some());
    let mut menu = CatalogMenu::new(snapshot.clone(), context, &preferences).unwrap();
    menu.open_class(MiiClass::Gunner);
    assert_eq!(menu.view(&preferences).panel.labels[1], "CPU: A");
    menu.step(1);
    menu.confirm(&mut preferences);
    let view = menu.view(&preferences);
    assert_eq!(view.disabled, [false]);
    assert!(view.panel.footer.contains("Imported presets unavailable"));
    menu.confirm(&mut preferences);
    assert_eq!(preferences.get(Role::Cpu, MiiClass::Gunner), Some(wanted));

    snapshot.findings.push(Finding::Source { slot_id: Some(9999), error: "native save unreadable".into() });
    assert!(!snapshot.is_complete());
    let mut menu = CatalogMenu::new(snapshot, context, &preferences).unwrap();
    menu.open_class(MiiClass::Gunner);
    menu.step(1);
    menu.confirm(&mut preferences);
    let view = menu.view(&preferences);
    assert!(view.disabled.iter().all(|disabled| *disabled));
    assert!(view.panel.footer.contains("Selection is unavailable"));
}

#[test]
fn imported_preset_name_is_shown_in_both_role_summaries_and_the_dropdown() {
    use training_mod_mii::draft::{Draft, Library};
    let native = catalog();
    let template = native.configs.iter().find(|config| config.slot_id == 20).unwrap();
    let mut draft = Draft::from_template(template).unwrap();
    draft.set_specials([1, 0, 0, 0]).unwrap();
    let configuration = draft.configuration().unwrap();
    let id = configuration.canonical();
    let mut library = Library::default();
    library.import(configuration, "Replay Gunner", &native).unwrap();
    let snapshot = native.with_library(&library);
    let mut preferences = Preferences::default()
        .with(Role::Player, MiiClass::Gunner, Some(id)).unwrap()
        .with(Role::Cpu, MiiClass::Gunner, Some(id)).unwrap();
    let before = preferences.clone();
    let context = Context { epoch: 1, original: PlayerOrigin::NotMii,
        active: Active { player: PlayerLive::NotMii, cpu: None }, cpu_original: None };
    let mut menu = CatalogMenu::new(snapshot, context, &preferences).unwrap();
    menu.open_class(MiiClass::Gunner);
    let view = menu.view(&preferences);
    assert_eq!(&view.panel.labels[..2], ["Player: Replay Gunner", "CPU: Replay Gunner"]);
    menu.step(1);
    menu.confirm(&mut preferences);
    let view = menu.view(&preferences);
    assert!(view.panel.labels[view.panel.selected].contains("Replay Gunner [2111]"));
    assert!(!view.disabled[view.panel.selected]);
    menu.confirm(&mut preferences);
    assert_eq!(preferences, before);
}

#[test]
fn independent_dropdowns_keep_slot0_fallback_and_emit_only_at_actual_close() {
    let snapshot = catalog();
    let original = canonical(&snapshot, 2622);
    let alternative = canonical(&snapshot, 2700);
    let mut app = app();
    app.mii_preferences = Preferences::default().with(Role::Player, MiiClass::Swordfighter, Some(original)).unwrap();
    let persisted_player = serde_json::to_value(&app.mii_preferences).unwrap()["player"].clone();
    let ordinary_settings = app.current_settings_to_json();
    let context = Context { epoch: 9, original: PlayerOrigin::Mii(Origin(original)),
        active: Active { player: PlayerLive::Origin, cpu: Some((MiiClass::Swordfighter, original)) },
        cpu_original: None };
    assert!(app.mount_mii_catalog(snapshot, context));
    assert_eq!(app.tabs.items[0].submenus.iter().map(|s| s.id).collect::<Vec<_>>(),
        ["solo_popo_original", "mii_brawler_config", "mii_sword_config", "mii_gunner_config"]);
    assert_eq!(app.current_settings_to_json(), ordinary_settings, "UI launchers do not become menu values");
    app.tabs.items[0].submenus.select(0, 2);
    app.on_a(); // Sword class
    app.on_a(); // Player dropdown, fallback opens at alternative
    let view = app.mii_view().unwrap();
    assert_eq!(view.panel.labels.len(), 2, "duplicate canonical config collapsed upstream");
    assert!(view.panel.labels[0].starts_with("1. "));
    assert!(!view.panel.labels[0].contains("2622"));
    assert_eq!(view.disabled, [true, false]);
    assert_eq!(view.panel.selected, 1);
    app.on_left(); app.on_a(); // protected original refused
    assert_eq!(app.mii_preferences.get(Role::Player, MiiClass::Swordfighter), Some(original));
    app.on_b(); // back to class, not a menu-close transaction
    app.on_right(); app.on_a(); // CPU dropdown
    assert_eq!(app.mii_view().unwrap().disabled, [false, false]);
    app.on_right(); app.on_a(); // choose CPU alternative
    app.on_b(); // back to Extras
    assert!(app.page == AppPage::SUBMENU);
    assert!(app.take_pending_mii_selection(9).is_none());
    app.on_b(); // actual menu close
    let intent = app.take_pending_mii_selection(9).unwrap();
    assert_eq!(intent.context, context);
    assert!(matches!(intent.actions.as_slice(), [Action::ReloadCpu(choice)] if choice.canonical == alternative));
    assert!(app.take_pending_mii_selection(9).is_none(), "one-shot intent");
    app.on_start();
    assert!(app.take_pending_mii_selection(9).is_none(), "already closed menu cannot rearm an intent");
    assert_eq!(serde_json::to_value(&app.mii_preferences).unwrap()["player"], persisted_player);
    let envelope: serde_json::Value = serde_json::from_str(&app.get_serialized_settings_with_defaults()).unwrap();
    assert_eq!(envelope["mii_preferences"]["cpu"][1], alternative.to_hex());
}

#[test]
fn departure_discards_pending_and_incomplete_catalog_cannot_change_preferences() {
    let mut snapshot = catalog();
    let original = canonical(&snapshot, 2622);
    let context = Context { epoch: 11, original: PlayerOrigin::NotMii,
        active: Active { player: PlayerLive::NotMii, cpu: Some((MiiClass::Swordfighter, original)) },
        cpu_original: None };
    let mut app = app();
    assert!(app.mount_mii_catalog(snapshot.clone(), context));
    app.tabs.items[0].submenus.select(0, 2);
    app.on_a(); app.on_right(); app.on_a(); app.on_right(); app.on_a(); app.on_start();
    assert!(app.take_pending_mii_selection(12).is_none(), "stale epoch intent refused");
    app.cancel_mii_catalog();
    assert_eq!(app.tabs.items[0].submenus.len(), 1);
    assert!(app.mii_view().is_none());
    snapshot.findings.push(Finding::Source { slot_id: Some(9999), error: "unreadable saved header".into() });
    let preferences = app.mii_preferences.clone();
    assert!(app.mount_mii_catalog(snapshot, context));
    app.tabs.items[0].submenus.select(0, 2);
    app.on_a(); app.on_right(); app.on_a(); app.on_right(); app.on_a();
    assert_eq!(app.mii_preferences, preferences);
    let view = app.mii_view().unwrap();
    assert!(view.disabled.iter().all(|disabled| *disabled));
    assert_eq!(view.panel.footer, "1 saved Mii configuration(s) could not be read. Selection is unavailable.");
    assert!(!view.panel.footer.contains("9999"));
    app.cancel_mii_catalog();
    assert!(app.take_pending_mii_selection(11).is_none());
}

#[test]
fn long_saved_catalog_pages_select_canonical_values_beyond_first_screen() {
    let snapshot = build_snapshot((1..=40).map(|id| slot(id, MiiClass::Gunner, id as u8)).collect());
    let wanted = canonical(&snapshot, 40);
    let mut preferences = Preferences::default();
    let context = Context { epoch: 1, original: PlayerOrigin::NotMii,
        active: Active { player: PlayerLive::NotMii, cpu: None }, cpu_original: None };
    let mut menu = CatalogMenu::new(snapshot, context, &preferences).unwrap();
    menu.open_class(MiiClass::Gunner);
    menu.confirm(&mut preferences);
    for _ in 0..39 { menu.step(1); }
    let view = menu.view(&preferences);
    assert_eq!((view.panel.labels.len(), view.panel.selected), (8, 7));
    menu.confirm(&mut preferences);
    assert_eq!(preferences.get(Role::Player, MiiClass::Gunner), Some(wanted));
    assert_eq!(preferences.get(Role::Cpu, MiiClass::Gunner), None);
    assert!(menu.close(&preferences).is_none(), "inactive classes produce no reload intent");
}

#[cfg(feature = "mii-saved-alpha")]
#[test]
fn saved_class_cycle_admission_preserves_origin_and_refuses_other_classes() {
    for class in [MiiClass::Brawler, MiiClass::Gunner] {
        let snapshot = build_snapshot(vec![slot(10, class, 1), slot(20, class, 2)]);
        let origin = canonical(&snapshot, 10);
        let mut app = app();
        let context = Context { epoch: 9, original: PlayerOrigin::Mii(Origin(origin)),
            active: Active { player: PlayerLive::Origin, cpu: None },
            cpu_original: None };
        assert!(app.mount_mii_catalog(snapshot.clone(), context));
        assert!(!app.roster_excluded(training_mod_tui::roster_grid::Role::Player, class as u32));
        assert!(app.roster_excluded(training_mod_tui::roster_grid::Role::Player, MiiClass::Swordfighter as u32));
        assert!(app.roster_excluded(training_mod_tui::roster_grid::Role::Cpu, class as u32));
        let ordinary = Context { original: PlayerOrigin::NotMii,
            active: Active { player: PlayerLive::NotMii, cpu: None }, ..context };
        app.cancel_mii_catalog();
        assert!(app.mount_mii_catalog(snapshot, ordinary));
        assert!(app.roster_excluded(training_mod_tui::roster_grid::Role::Player, class as u32));
    }
}

#[cfg(feature="mii-ordinary-player-alpha")]
#[test]
fn ordinary_cycle_cell_requires_captured_player_context_and_never_enables_cpu_origin_changes() {
    let snapshot=catalog();
    let mut app=app();
    let context=Context {epoch:19,original:PlayerOrigin::NotMii,
        active:Active {player:PlayerLive::NotMii,cpu:None},cpu_original:None};
    assert!(app.mount_mii_catalog(snapshot.clone(),context));
    assert!(app.roster_excluded(training_mod_tui::roster_grid::Role::Player,MiiClass::Swordfighter as u32));
    app.cancel_mii_catalog();
    assert!(app.mount_mii_catalog(snapshot,Context {active:Active {
        player:PlayerLive::Origin,cpu:None},..context}));
    assert!(!app.roster_excluded(training_mod_tui::roster_grid::Role::Player,MiiClass::Swordfighter as u32));
    assert!(app.roster_excluded(training_mod_tui::roster_grid::Role::Cpu,MiiClass::Swordfighter as u32));
}


#[cfg(feature="mii-ordinary-player-alpha")]
#[test]
fn ordinary_dropdown_routes_active_player_on_close_and_keeps_slot_zero_unchanged() {
    let snapshot=build_snapshot(vec![
        slot(2622,MiiClass::Swordfighter,1),
        slot(2700,MiiClass::Swordfighter,2),
        slot(2900,MiiClass::Swordfighter,3),
    ]);
    let origin=canonical(&snapshot,2622);
    let active=canonical(&snapshot,2700);
    let wanted=canonical(&snapshot,2900);
    let before=Preferences::default().with(Role::Player,MiiClass::Swordfighter,Some(active)).unwrap();
    let context=Context {epoch:20,original:PlayerOrigin::NotMii,
        active:Active {player:PlayerLive::Cycled {class:MiiClass::Swordfighter,canonical:active},
            cpu:Some((MiiClass::Swordfighter,origin))},cpu_original:None};
    let mut preferences=before.clone();
    let mut menu=CatalogMenu::new(snapshot.clone(),context,&before).unwrap();
    menu.open_class(MiiClass::Swordfighter);
    menu.confirm(&mut preferences); // Player dropdown starts on the current preference.
    assert!(menu.view(&preferences).panel.footer.contains("active Player class updates on menu close"));
    menu.step(1);
    menu.confirm(&mut preferences); // Player selects2900; no reload is submitted here.
    menu.step(1);
    menu.confirm(&mut preferences); // CPU dropdown starts on2622.
    menu.step(2);
    menu.confirm(&mut preferences);
    assert_eq!(preferences.get(Role::Player,MiiClass::Swordfighter),Some(wanted));
    let retained=preferences.clone();
    let pending=menu.close(&preferences).unwrap();
    assert_eq!(pending.context,context);
    assert_eq!(pending.actions.len(),2);
    assert!(matches!(pending.actions[0],Action::ReloadPlayer(c) if c.canonical==wanted));
    assert!(matches!(pending.actions[1],Action::ReloadCpu(c) if c.canonical==wanted));
    let request=training_mod_mii::ordinary_player::Request::from_menu(
        context.epoch,context.epoch,context.original,context.active.player,
        Some(active),pending.actions[0]).unwrap();
    assert!(request.current(context.epoch,Some(active)));
    assert!(!request.current(context.epoch,Some(wanted)));
    assert_eq!(preferences,retained);
    let next=training_mod_mii::selection::resolve(&snapshot,&preferences,Role::Player,
        MiiClass::Swordfighter,PlayerOrigin::NotMii).unwrap();
    assert_eq!(next.slot_id.unwrap(),2900); // the next original -> saved Cycle uses this persisted choice

    // An unchanged close and a return to immutable ordinary slot0 do not reload Player.
    let unchanged=CatalogMenu::new(snapshot.clone(),context,&preferences).unwrap();
    assert!(unchanged.close(&preferences).is_none());
    let at_origin=CatalogMenu::new(snapshot.clone(),Context {active:Active {
        player:PlayerLive::Origin,cpu:None},..context},&before).unwrap();
    assert!(at_origin.close(&preferences).is_none());

    // Saved-origin behavior still produces both actions through its existing owner.
    let saved=CatalogMenu::new(snapshot,Context {
        original:PlayerOrigin::Mii(Origin(origin)),..context},&before).unwrap();
    let actions=saved.close(&preferences).unwrap().actions;
    assert_eq!(actions.len(),2);
    assert!(matches!(actions[0],Action::ReloadPlayer(c) if c.canonical==wanted));
    assert!(matches!(actions[1],Action::ReloadCpu(c) if c.canonical==wanted));
}

#[cfg(feature="mii-ordinary-player-alpha")]
#[test]
fn failed_cpu_recapture_keeps_ordinary_player_menu_but_never_queues_cpu() {
    let snapshot = catalog();
    let original = canonical(&snapshot, 2622);
    let alternate = canonical(&snapshot, 2700);
    let ordinary = PlayerLive::Cycled { class: MiiClass::Swordfighter, canonical: original };
    let raw = Context { epoch: 30, original: PlayerOrigin::NotMii,
        active: Active { player: PlayerLive::NotMii, cpu: Some((MiiClass::Swordfighter, original)) },
        cpu_original: None };
    let mut capture = MenuCapture::closed();
    capture.begin();
    capture.finish(Role::Player, false); // ordinary adapter owns Player, not SavedRole
    assert!(capture.take_context(raw, Some(ordinary)).is_none(), "CPU capture still pending");
    capture.finish(Role::Cpu, false); // exact current CPU scope rejected after Player changed
    let context = capture.take_context(raw, Some(ordinary)).unwrap();
    assert_eq!(context.active.cpu, None);
    assert_eq!(context.active.player, ordinary);
    let mut app = app();
    app.mii_preferences = Preferences::default()
        .with(Role::Player, MiiClass::Swordfighter, Some(original)).unwrap()
        .with(Role::Cpu, MiiClass::Swordfighter, Some(original)).unwrap();
    assert!(app.mount_mii_catalog(snapshot.clone(), context));
    assert_eq!(app.tabs.items[0].submenus.len(), 4, "all three class pages remain mounted");
    app.tabs.items[0].submenus.select(0, 2);
    app.on_a(); app.on_a(); app.on_right(); app.on_a(); // Player alternate
    app.on_right(); app.on_a(); app.on_right(); app.on_a(); // CPU preference only
    app.on_b(); app.on_b();
    let pending = app.take_pending_mii_selection(30).unwrap();
    assert!(matches!(pending.actions.as_slice(), [Action::ReloadPlayer(c)] if c.canonical == alternate));
    assert_eq!(app.mii_preferences.get(Role::Cpu, MiiClass::Swordfighter), Some(alternate));
    assert!(training_mod_mii::ordinary_player::Request::from_menu(30, 30,
        context.original, context.active.player, Some(original), pending.actions[0]).is_some());

    // A later opening cannot borrow either the previous ordinary context or saved validity.
    app.cancel_mii_catalog();
    capture.begin();
    capture.finish(Role::Player, false);
    assert!(capture.take_context(Context { epoch: 31, ..raw }, None).is_none());
    capture.finish(Role::Cpu, false);
    assert!(capture.take_context(Context { epoch: 31, ..raw }, None).is_none());
    assert_eq!(app.tabs.items[0].submenus.len(), 1);
    capture.begin();
    capture.finish(Role::Cpu, true);
    capture.cancel(); // departure/destruction cancels partial capture too
    capture.finish(Role::Player, true);
    assert!(capture.take_context(raw, Some(ordinary)).is_none());
}

#[test]
fn failed_saved_player_recapture_protects_origin_and_keeps_valid_cpu_independent() {
    let snapshot = build_snapshot(vec![slot(2622, MiiClass::Swordfighter, 1),
        slot(2700, MiiClass::Swordfighter, 2), slot(2900, MiiClass::Swordfighter, 3)]);
    let original = canonical(&snapshot, 2622);
    let alternate = canonical(&snapshot, 2700);
    let later = canonical(&snapshot, 2900);
    let raw = Context { epoch: 40, original: PlayerOrigin::Mii(Origin(original)),
        active: Active { player: PlayerLive::Cycled { class: MiiClass::Swordfighter, canonical: alternate },
            cpu: Some((MiiClass::Swordfighter, original)) },
        cpu_original: None };
    let mut capture = MenuCapture::closed();
    capture.begin();
    capture.finish(Role::Player, false);
    capture.finish(Role::Cpu, true);
    // Even a supplied ordinary context cannot replace a protected saved original.
    let context = capture.take_context(raw, Some(PlayerLive::Origin)).unwrap();
    assert_eq!(context.original, PlayerOrigin::Unresolved);
    assert_eq!(context.active.player, PlayerLive::NotMii);
    let mut app = app();
    app.mii_preferences = Preferences::default()
        .with(Role::Player, MiiClass::Swordfighter, Some(original)).unwrap()
        .with(Role::Cpu, MiiClass::Swordfighter, Some(original)).unwrap();
    assert!(app.mount_mii_catalog(snapshot.clone(), context));
    app.tabs.items[0].submenus.select(0, 2);
    app.on_a(); app.on_a(); // Player dropdown
    assert!(app.mii_view().unwrap().disabled.iter().all(|disabled| *disabled));
    app.on_a(); // cannot select the protected original or any other Player row
    assert_eq!(app.mii_preferences.get(Role::Player, MiiClass::Swordfighter), Some(original));
    app.on_b(); app.on_right(); app.on_a(); app.on_right(); app.on_a();
    app.on_b(); app.on_b();
    let pending = app.take_pending_mii_selection(40).unwrap();
    assert!(matches!(pending.actions.as_slice(), [Action::ReloadCpu(c)] if c.canonical == alternate));

    // Independently revalidated saved Player can be exposed on a later menu; old CPU validity cannot.
    capture.begin();
    capture.finish(Role::Player, true);
    assert!(capture.take_context(raw, None).is_none());
    capture.finish(Role::Cpu, false);
    let context = capture.take_context(Context { epoch: 41, ..raw }, None).unwrap();
    assert_eq!(context.original, raw.original);
    assert_eq!(context.active.cpu, None);
    let before = Preferences::default().with(Role::Player, MiiClass::Swordfighter, Some(alternate)).unwrap();
    let changed = before.clone().with(Role::Player, MiiClass::Swordfighter, Some(later)).unwrap();
    let menu = CatalogMenu::new(snapshot, context, &before).unwrap();
    assert!(matches!(menu.close(&changed).unwrap().actions.as_slice(), [Action::ReloadPlayer(c)] if c.canonical == later));
}

#[cfg(feature="mii-ordinary-cpu-alpha")]
#[test]
fn ordinary_cpu_cycle_is_explicit_and_preferences_do_not_submit_saved_origin_reload() {
    use training_mod_tui::mii_menu::MenuCapture;
    let snapshot=catalog();
    let selected=canonical(&snapshot,2700);
    let mut capture=MenuCapture::closed();capture.begin();
    capture.finish(Role::Player,false);capture.finish(Role::Cpu,false);
    let raw=Context {epoch:17,original:PlayerOrigin::NotMii,
        active:Active {player:PlayerLive::NotMii,cpu:None},cpu_original:None};
    let context=capture.take_context_with_cpu(raw,None,true).unwrap();
    assert_eq!(context.original,PlayerOrigin::NotMii);
    assert_eq!(context.active.cpu,None); // never fabricate a saved-origin role
    let mut app=app();
    assert!(app.mount_mii_catalog(snapshot,context));
    let cpu=training_mod_tui::roster_grid::Role::Cpu;
    assert!(app.roster_excluded(cpu,0x49)); // catalog alone is not admission
    app.qualify_ordinary_cpu_cycle(true);
    assert!(!app.roster_excluded(cpu,0x49));
    assert!(!app.roster_excluded(cpu,0x4a));
    assert!(!app.roster_excluded(cpu,0x48)); // native descriptor qualified; runtime acceptance is separate
    app.tabs.items[0].submenus.select(0,2);
    app.on_a();app.on_a();app.on_right();app.on_a(); // future Player preference is editable
    assert_eq!(app.mii_preferences.get(Role::Player,MiiClass::Swordfighter),Some(selected));
    app.on_right();app.on_a();app.on_right();app.on_a(); // CPU preference remains independent
    assert_eq!(app.mii_preferences.get(Role::Cpu,MiiClass::Swordfighter),Some(selected));
    app.on_start();
    assert!(app.take_pending_mii_selection(17).is_none()); // preference for next cycle only
    app.qualify_ordinary_cpu_cycle(false);
    assert!(app.roster_excluded(cpu,0x49));
}

#[cfg(feature="mii-ordinary-player-alpha")]
#[test]
fn reopen_after_ordinary_cycle_resets_then_captures_publishes_and_redraws() {
    let snapshot = catalog();
    let active = canonical(&snapshot, 2700);
    let live = PlayerLive::Cycled { class: MiiClass::Swordfighter, canonical: active };
    let raw = Context { epoch: 50, original: PlayerOrigin::NotMii,
        active: Active { player: PlayerLive::NotMii, cpu: None }, cpu_original: None };
    let mut app = app();
    app.mii_preferences = Preferences::default()
        .with(Role::Player, MiiClass::Swordfighter, Some(active)).unwrap();
    let preferences = app.mii_preferences.clone();
    let initial = Context { active: Active { player: PlayerLive::Origin, cpu: None }, ..raw };
    assert!(app.mount_mii_catalog(snapshot.clone(), initial));
    app.on_b();
    assert!(app.page == AppPage::CLOSE);

    // Historical caller order: capture is consumed while the old App is closed.
    let mut capture = MenuCapture::closed();
    capture.begin();
    capture.finish(Role::Player, false); // ordinary adapter, not SavedRole
    capture.finish(Role::Cpu, false); // old saved CPU scope cannot qualify
    let early = capture.take_context(raw, Some(live)).unwrap();
    assert!(!app.mount_mii_catalog(snapshot.clone(), early));
    app.open_settings_for_player("{}", Some(9));
    app.page = AppPage::SUBMENU;
    assert!(capture.take_context(raw, Some(live)).is_none());
    assert_eq!(app.tabs.items[0].submenus.len(), 1, "consumed early capture has no pages");

    // Production order: cancel/advance opening, reset App, then arm capture.
    capture.cancel();
    let current_epoch = 51;
    app.open_settings_for_player("{}", Some(9));
    app.page = AppPage::SUBMENU;
    let mut redraw = false; // initial render has already consumed the opening event
    assert!(!app.publish_mii_catalog(snapshot.clone(), early, current_epoch, || redraw = true));
    assert!(!redraw, "old completed capture cannot mount in the reset/arm gap");
    capture.begin();
    capture.finish(Role::Player, false);
    assert!(capture.take_context(Context { epoch: current_epoch, ..raw }, Some(live)).is_none());
    capture.finish(Role::Cpu, false);
    let current = capture.take_context(Context { epoch: current_epoch, ..raw }, Some(live)).unwrap();
    assert!(app.publish_mii_catalog(snapshot.clone(), current, current_epoch, || redraw = true));
    assert!(redraw, "successful late mount invalidates the event-driven renderer");
    assert_eq!(app.tabs.items[0].submenus.iter().map(|s| s.id).collect::<Vec<_>>(),
        ["solo_popo_original", "mii_brawler_config", "mii_sword_config", "mii_gunner_config"]);
    assert_eq!(app.original_player_kind, Some(9));
    assert_eq!(app.mii_preferences, preferences);
    redraw = false;
    assert!(!app.publish_mii_catalog(snapshot, current, current_epoch, || redraw = true));
    assert!(!redraw, "duplicate mount produces no event or duplicate pages");
    app.tabs.items[0].submenus.select(0, 2);
    app.on_a();
    assert!(app.mii_view().unwrap().panel.footer.contains("active Player class updates on menu close"));
    app.on_b(); app.on_b();
    assert!(app.take_pending_mii_selection(current_epoch).is_none(), "unchanged reopen/close reloads nothing");
}

#[cfg(feature="mii-ordinary-player-alpha")]
#[test]
fn unified_saved_origin_context_keeps_slot_zero_grey_and_cpu_independent() {
    let snapshot=catalog();let origin=canonical(&snapshot,2622);
    let alternate=canonical(&snapshot,2700);
    let mut capture=MenuCapture::closed();capture.begin();
    capture.finish(Role::Player,false);capture.finish(Role::Cpu,false);
    let raw=Context{epoch:50,original:PlayerOrigin::NotMii,
        active:Active{player:PlayerLive::NotMii,cpu:None},cpu_original:None};
    let context=capture.take_owned_context(raw,
        Some((PlayerOrigin::Mii(Origin(origin)),PlayerLive::Origin)),
        Some(Some((MiiClass::Swordfighter,origin)))).unwrap();
    assert_eq!(context.original,PlayerOrigin::Mii(Origin(origin)));
    let preferences=Preferences::default();
    let rows=training_mod_mii::selection::dropdown(&snapshot,&preferences,Role::Player,
        MiiClass::Swordfighter,context.original);
    assert!(rows.iter().find(|r|r.canonical==origin).unwrap().greyed);
    assert!(!rows.iter().find(|r|r.canonical==alternate).unwrap().greyed);
    let menu=CatalogMenu::new(snapshot.clone(),context,&preferences).unwrap();
    assert!(menu.alpha_player_cycle(0x49));
    assert!(menu.alpha_player_cycle(0x4a));
    let changed=preferences.clone().with(Role::Player,MiiClass::Swordfighter,Some(alternate)).unwrap()
        .with(Role::Cpu,MiiClass::Swordfighter,Some(alternate)).unwrap();
    // At exact Player slot0 only CPU applies on close; Player's preference waits for cycle.
    assert!(matches!(menu.close(&changed).unwrap().actions.as_slice(),[Action::ReloadCpu(_)]));
    capture.begin();capture.finish(Role::Player,false);capture.finish(Role::Cpu,false);
    let active=PlayerLive::Cycled{class:MiiClass::Swordfighter,canonical:alternate};
    let context=capture.take_owned_context(raw,
        Some((PlayerOrigin::Mii(Origin(origin)),active)),Some(None)).unwrap();
    assert_eq!(context.active.player,active);assert_eq!(context.active.cpu,None);
    capture.begin();capture.finish(Role::Player,false);capture.finish(Role::Cpu,false);
    assert!(capture.take_owned_context(raw,None,None).is_none());
}

#[cfg(feature="mii-managed-helper-alpha")]
#[test]
fn helper_hat_requires_both_captures_and_a_real_class_without_changing_preferences() {
    let snapshot=catalog();let prefs=Preferences::default();let before=prefs.clone();
    let context=Context {epoch:8,original:PlayerOrigin::NotMii,
        active:Active {player:PlayerLive::Origin,cpu:None},cpu_original:None};
    let mut menu=CatalogMenu::new(snapshot,context,&prefs).unwrap();
    assert!(!menu.alpha_helper_hat(0x48));menu.qualify_ordinary_cpu_cycle(true);
    assert!(menu.alpha_helper_hat(0x48));assert!(!menu.alpha_helper_hat(0x06));
    assert_eq!(prefs,before);
    let mut missing=CatalogMenu::new(build_snapshot(Vec::new()),context,&prefs).unwrap();
    missing.qualify_ordinary_cpu_cycle(true);assert!(!missing.alpha_helper_hat(0x48));
    let mut uncaptured=CatalogMenu::new(catalog(),Context {active:Active {
        player:PlayerLive::NotMii,cpu:None},..context},&prefs).unwrap();
    uncaptured.qualify_ordinary_cpu_cycle(true);assert!(!uncaptured.alpha_helper_hat(0x48));
    let mut app=app();
    app.mii_preferences=prefs;
    app.roster_purpose=training_mod_tui::roster_grid::RosterPurpose::KirbyHats;
    assert!(app.mount_mii_catalog(catalog(),context));
    assert!(app.hat_cell_disabled(0x48));
    app.qualify_ordinary_cpu_cycle(true);
    assert!(!app.hat_cell_disabled(0x48));
    assert!(app.hat_cell_disabled(0x06));
    for role in [training_mod_tui::roster_grid::Role::Player,training_mod_tui::roster_grid::Role::Cpu] {
        assert!(!app.roster_excluded(role,0x48));
    }
    assert_eq!(app.mii_preferences,before);
}
