//! Host before/after benchmark for the input-log renderer.
//!
//! The included renderer is production `input_log.rs` (baseline fixture or a
//! candidate snapshot). Pane search, text, and menu locks are shims. Counts and
//! CPU time are host-side. They are not emulator FPS.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use training_mod_consts::{InputDisplay, OnOff, TrainingModpackMenu, MENU};
use training_mod_sync::{assign, read};

mod common {
    pub mod consts {
        /// Fixture label, not the game's status-name table.
        pub fn status_display_name(fighter_kind: i32, status: i32) -> String {
            format!("k{fighter_kind}/s{status}")
        }
    }
}

mod menu {
    use training_mod_sync::RwLock;
    pub static QUICK_MENU_ACTIVE: RwLock<bool> = RwLock::new(false);
}

mod training {
    pub mod input_log {
        use std::collections::VecDeque;

        use skyline::nn::ui2d::ResColor;
        use training_mod_consts::{InputDisplay, MENU};
        use training_mod_sync::{LazyLock, RwLock, read};

        pub const NUM_LOGS: usize = 15;
        pub static DRAW_LOG_BASE_IDX: RwLock<usize> = RwLock::new(0);
        pub const YELLOW: ResColor = ResColor {
            r: 230,
            g: 180,
            b: 14,
            a: 0,
        };
        pub const WHITE: ResColor = ResColor {
            r: 255,
            g: 255,
            b: 255,
            a: 0,
        };
        const GREEN: ResColor = ResColor {
            r: 22,
            g: 156,
            b: 0,
            a: 0,
        };
        const RED: ResColor = ResColor {
            r: 153,
            g: 10,
            b: 10,
            a: 0,
        };
        const CYAN: ResColor = ResColor {
            r: 0,
            g: 255,
            b: 255,
            a: 0,
        };
        const BLUE: ResColor = ResColor {
            r: 0,
            g: 40,
            b: 108,
            a: 0,
        };
        const PURPLE: ResColor = ResColor {
            r: 100,
            g: 66,
            b: 202,
            a: 0,
        };

        #[derive(PartialEq, Eq, Debug, Copy, Clone)]
        pub enum DirectionStrength {
            None,
            Weak,
        }

        /// Fixture inputs. Button bits and stick angles stand in for the game mapper.
        #[derive(Copy, Clone)]
        pub struct InputLog {
            pub ttl: u32,
            pub frames: u32,
            pub status: i32,
            pub fighter_kind: i32,
            pub buttons: u32,
            pub lstick: u16,
            pub rstick: u16,
        }

        pub const STICK_NONE: u16 = u16::MAX;

        impl Default for InputLog {
            fn default() -> Self {
                Self {
                    ttl: 0,
                    frames: 0,
                    status: 0,
                    fighter_kind: 0,
                    buttons: 0,
                    lstick: STICK_NONE,
                    rstick: STICK_NONE,
                }
            }
        }

        impl InputLog {
            pub fn is_different(&self, other: &InputLog) -> bool {
                let menu = read(&MENU);
                let status_diff = menu.input_display_status.as_bool() && self.status != other.status;
                match menu.input_display {
                    InputDisplay::SMASH | InputDisplay::RAW => {
                        self.buttons != other.buttons
                            || self.lstick != other.lstick
                            || self.rstick != other.rstick
                            || status_diff
                    }
                    InputDisplay::STATUS => status_diff,
                    InputDisplay::NONE => false,
                }
            }

            pub fn binned_lstick(&self) -> (DirectionStrength, f32) {
                self.binned(self.lstick)
            }

            pub fn binned_rstick(&self) -> (DirectionStrength, f32) {
                self.binned(self.rstick)
            }

            fn binned(&self, angle: u16) -> (DirectionStrength, f32) {
                match read(&MENU).input_display {
                    InputDisplay::STATUS | InputDisplay::NONE => (DirectionStrength::None, 0.0),
                    InputDisplay::SMASH | InputDisplay::RAW => {
                        if angle == STICK_NONE {
                            (DirectionStrength::None, 0.0)
                        } else {
                            (DirectionStrength::Weak, angle as f32)
                        }
                    }
                }
            }

            pub fn button_icons(&self) -> VecDeque<(&'static str, ResColor)> {
                match read(&MENU).input_display {
                    InputDisplay::STATUS | InputDisplay::NONE => VecDeque::new(),
                    InputDisplay::SMASH => icons_from_bits(self.buttons, false),
                    InputDisplay::RAW => icons_from_bits(self.buttons, true),
                }
            }
        }

        fn icons_from_bits(bits: u32, front: bool) -> VecDeque<(&'static str, ResColor)> {
            const TABLE: [(u32, &str, ResColor); 8] = [
                (1 << 0, "a", GREEN),
                (1 << 1, "b", RED),
                (1 << 2, "x", CYAN),
                (1 << 3, "y", CYAN),
                (1 << 4, "lb", BLUE),
                (1 << 5, "rb", BLUE),
                (1 << 6, "zl", PURPLE),
                (1 << 7, "zr", PURPLE),
            ];
            let mut icons = VecDeque::new();
            for (bit, name, color) in TABLE {
                if bits & bit != 0 {
                    if front {
                        icons.push_front((name, color));
                    } else {
                        icons.push_back((name, color));
                    }
                }
            }
            icons
        }

        pub static P1_INPUT_LOGS: LazyLock<RwLock<[InputLog; NUM_LOGS]>> =
            LazyLock::new(|| RwLock::new([InputLog::default(); NUM_LOGS]));
    }

    pub mod ui {
        use skyline::nn::ui2d::{Pane, PaneFlag};

        pub mod menu {
            use training_mod_sync::RwLock;
            pub static VANILLA_MENU_ACTIVE: RwLock<bool> = RwLock::new(false);
        }

        /// Same alpha curve as `src/training/ui/mod.rs`.
        pub fn fade_out(pane: &mut Pane, current_frame: u32, total_frames: u32) {
            skyline::note_fade();
            if current_frame >= total_frames {
                pane.alpha = 255;
                pane.global_alpha = 255;
            } else if current_frame > 0 {
                let alpha = ((255.0 / (total_frames as f32 + 1.0).log10())
                    * (current_frame as f32 + 1.0).log10()) as u8;
                pane.alpha = alpha;
                pane.global_alpha = alpha;
            } else {
                pane.alpha = 0;
                pane.global_alpha = 0;
            }
            let _ = PaneFlag::Visible;
        }

        pub trait PaneExt {
            unsafe fn find_pane_by_name_recursive_expect(&self, name: &str) -> &mut Pane;
        }

        impl PaneExt for Pane {
            unsafe fn find_pane_by_name_recursive_expect(&self, name: &str) -> &mut Pane {
                self.find_pane_by_name_recursive(name)
                    .unwrap_or_else(|| panic!("Could not find pane {}", name))
            }
        }

        pub mod input_log {
            include!(concat!(env!("INPUT_DISPLAY_RENDERER")));
        }
    }
}

struct CountingAlloc;

static ALLOC_COUNT: AtomicU64 = AtomicU64::new(0);
static ALLOC_BYTES: AtomicU64 = AtomicU64::new(0);
static ALLOC_ARMED: AtomicU64 = AtomicU64::new(0);

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note_alloc(layout.size());
        System.alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout)
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note_alloc(new_size);
        System.realloc(ptr, layout, new_size)
    }
}

fn note_alloc(bytes: usize) {
    if ALLOC_ARMED.load(Ordering::Relaxed) != 0 {
        ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        ALLOC_BYTES.fetch_add(bytes as u64, Ordering::Relaxed);
    }
}

#[global_allocator]
static GLOBAL_ALLOC: CountingAlloc = CountingAlloc;

fn arm_allocs() {
    ALLOC_COUNT.store(0, Ordering::Relaxed);
    ALLOC_BYTES.store(0, Ordering::Relaxed);
    ALLOC_ARMED.store(1, Ordering::Relaxed);
}

fn disarm_allocs() {
    ALLOC_ARMED.store(0, Ordering::Relaxed);
}

fn invalidate_renderer() {
    crate::training::ui::input_log::invalidate();
}

fn draw() {
    unsafe {
        crate::training::ui::input_log::draw(&*skyline::nn::ui2d::root_ptr());
    }
}

fn set_menu(display: InputDisplay, status: bool) {
    assign(
        &MENU,
        TrainingModpackMenu {
            input_display: display,
            input_display_status: if status { OnOff::ON } else { OnOff::OFF },
        },
    );
}

fn reset_world() {
    skyline::nn::ui2d::reset_panes();
    assign(
        &training::input_log::P1_INPUT_LOGS,
        [training::input_log::InputLog::default(); training::input_log::NUM_LOGS],
    );
    assign(&training::input_log::DRAW_LOG_BASE_IDX, 0usize);
    set_menu(InputDisplay::SMASH, false);
    assign(&menu::QUICK_MENU_ACTIVE, false);
    assign(&training::ui::menu::VANILLA_MENU_ACTIVE, false);
    invalidate_renderer();
}

fn put_logs(logs: [training::input_log::InputLog; 15]) {
    assign(&training::input_log::P1_INPUT_LOGS, logs);
}

fn head_log(
    buttons: u32,
    ttl: u32,
    frames: u32,
    status: i32,
    fighter_kind: i32,
    lstick: u16,
    rstick: u16,
) -> training::input_log::InputLog {
    training::input_log::InputLog {
        ttl,
        frames,
        status,
        fighter_kind,
        buttons,
        lstick,
        rstick,
    }
}

fn filled(buttons: u32, lstick: u16, rstick: u16) -> [training::input_log::InputLog; 15] {
    let none = training::input_log::STICK_NONE;
    let mut logs = [training::input_log::InputLog::default(); 15];
    for index in 0..15 {
        logs[index] = if index == 0 {
            head_log(buttons, 500, 30, 100, 13, lstick, rstick)
        } else {
            head_log(
                1 << ((index % 8) as u32),
                500 - (index as u32) * 10,
                30 - index as u32,
                100 + index as i32,
                13,
                none,
                none,
            )
        };
    }
    logs
}

fn rotate_front(new_head: training::input_log::InputLog) {
    let mut logs = read(&training::input_log::P1_INPUT_LOGS);
    for index in (1..15).rev() {
        logs[index] = logs[index - 1];
    }
    logs[0] = new_head;
    put_logs(logs);
    let base = read(&training::input_log::DRAW_LOG_BASE_IDX);
    assign(&training::input_log::DRAW_LOG_BASE_IDX, (base + 1) % 15);
}

// Establish the history through actual renderer calls. Each physical row must
// have been the newest row once, as it is during play; assigning 15 live logs
// to a fresh layout otherwise leaves the baseline's historical icons uninitialised.
fn prime_history() {
    let logs = read(&training::input_log::P1_INPUT_LOGS);
    let base = read(&training::input_log::DRAW_LOG_BASE_IDX);
    for index in (0..15).rev() {
        let mut pending = [training::input_log::InputLog::default(); 15];
        pending[0] = logs[index];
        put_logs(pending);
        assign(&training::input_log::DRAW_LOG_BASE_IDX, (base + 15 - index) % 15);
        draw();
    }
    put_logs(logs);
    assign(&training::input_log::DRAW_LOG_BASE_IDX, base);
    draw();
}

fn capture(scenario: &str, step: &str) {
    skyline::reset_counters();
    arm_allocs();
    draw();
    disarm_allocs();
    let counters = skyline::counters();
    println!("STEP scenario={scenario} step={step}");
    println!("lookup_calls={}", counters.lookup_calls);
    println!("lookup_visits={}", counters.lookup_visits);
    println!("visibility_writes={}", counters.visibility_writes);
    println!("text_writes={}", counters.text_writes);
    println!("color_writes={}", counters.color_writes);
    println!("fade_writes={}", counters.fade_writes);
    println!("allocs={}", ALLOC_COUNT.load(Ordering::Relaxed));
    println!("alloc_bytes={}", ALLOC_BYTES.load(Ordering::Relaxed));
    println!("STATE_VISIBLE_BEGIN");
    print!("{}", skyline::nn::ui2d::visible_state());
    println!("STATE_VISIBLE_END");
    println!("STATE_FULL_BEGIN");
    print!("{}", skyline::nn::ui2d::full_state());
    println!("STATE_FULL_END");
    println!("ENDSTEP");
}

fn time_case(scenario: &str, setup: &dyn Fn(), body: &dyn Fn()) {
    disarm_allocs();
    let iters = 20u32;
    for _ in 0..2 {
        setup();
        prime_history();
        body();
    }
    let mut total = std::time::Duration::from_nanos(0);
    for _ in 0..iters {
        setup();
        prime_history();
        let started = Instant::now();
        body();
        total += started.elapsed();
    }
    let nanos = total.as_nanos();
    println!(
        "TIMING scenario={scenario} iters={iters} cpu_ns_total={nanos} cpu_ns_per={}",
        nanos / u128::from(iters)
    );
}

fn main() {
    skyline::nn::ui2d::ensure_tree();
    let _ = read(&training::input_log::P1_INPUT_LOGS);
    let _ = read(&MENU);
    let _ = read(&menu::QUICK_MENU_ACTIVE);
    let _ = read(&training::ui::menu::VANILLA_MENU_ACTIVE);
    reset_world();
    draw();
    reset_world();

    println!("kind=host-bench");
    println!("not=emulator-fps");
    println!("label={}", env!("INPUT_DISPLAY_SOURCE_LABEL"));
    println!(
        "variant={}",
        if cfg!(bench_candidate) {
            "candidate"
        } else {
            "baseline"
        }
    );
    println!("opt_level=3");
    println!("tree_nodes={}", skyline::nn::ui2d::node_count());
    println!("tree=root/TrModInputLog/15 rows/(Frame,Status,Input0-4 x 37 icons)");
    println!("input_mapping=fixture-stubs");
    println!("status_names=fixture-stubs");

    steady_repeat();
    steady_tick();
    changing_buttons();
    ring_wrap();
    rapid_input();
    icon_color_and_clear();
    capped_frame_count();
    mode_cycle();
    status_toggle();
    hide_reopen();
    ttl_fade();
    ttl_decay();
    ring_rotate();
    late_head_tick();
    reset_logs();
    reregister();
    println!("DONE");
}

fn steady_repeat() {
    let logs = filled(1 << 0 | 1 << 1, training::input_log::STICK_NONE, training::input_log::STICK_NONE);
    let setup = || {
        reset_world();
        put_logs(logs);
    };
    setup();
    prime_history();
    capture("steady_repeat", "draw1");
    capture("steady_repeat", "draw2");
    time_case("steady_repeat", &setup, &|| {
        draw();
        draw();
    });
}

fn steady_tick() {
    let none = training::input_log::STICK_NONE;
    let setup = || {
        reset_world();
        put_logs(filled(1 << 0, none, none));
    };
    let body = || {
        draw();
        let mut logs = read(&training::input_log::P1_INPUT_LOGS);
        logs[0].ttl = 499;
        logs[0].frames = 31;
        put_logs(logs);
        draw();
    };
    setup();
    prime_history();
    capture("steady_tick", "draw1");
    let mut logs = read(&training::input_log::P1_INPUT_LOGS);
    logs[0].ttl = 499;
    logs[0].frames = 31;
    put_logs(logs);
    capture("steady_tick", "draw2");
    time_case("steady_tick", &setup, &body);
}

fn changing_buttons() {
    let none = training::input_log::STICK_NONE;
    let setup = || {
        reset_world();
        put_logs(filled(1 << 0, none, none));
    };
    let body = || {
        draw();
        let mut logs = read(&training::input_log::P1_INPUT_LOGS);
        logs[0].buttons = (1 << 0) | (1 << 1) | (1 << 2) | (1 << 3);
        logs[0].lstick = 90;
        logs[0].rstick = 0;
        logs[0].frames = 32;
        put_logs(logs);
        draw();
    };
    setup();
    prime_history();
    capture("changing_buttons", "before");
    let mut logs = read(&training::input_log::P1_INPUT_LOGS);
    logs[0].buttons = (1 << 0) | (1 << 1) | (1 << 2) | (1 << 3);
    logs[0].lstick = 90;
    logs[0].rstick = 0;
    logs[0].frames = 32;
    put_logs(logs);
    capture("changing_buttons", "after");
    time_case("changing_buttons", &setup, &body);
}

fn ring_wrap() {
    let none = training::input_log::STICK_NONE;
    let setup = || {
        reset_world();
        assign(&training::input_log::DRAW_LOG_BASE_IDX, 14usize);
        put_logs(filled(1 << 0, none, none));
    };
    let wrapped = head_log(1 << 1, 480, 1, 200, 13, none, none);
    let body = || {
        draw();
        rotate_front(wrapped);
        draw();
    };
    setup();
    prime_history();
    capture("ring_wrap", "base14");
    rotate_front(wrapped);
    capture("ring_wrap", "wrapped");
    time_case("ring_wrap", &setup, &body);
}

fn mode_cycle() {
    let none = training::input_log::STICK_NONE;
    let setup = || {
        reset_world();
        set_menu(InputDisplay::SMASH, false);
        put_logs(filled((1 << 0) | (1 << 1) | (1 << 2), none, none));
    };
    let body = || {
        set_menu(InputDisplay::SMASH, false);
        draw();
        set_menu(InputDisplay::RAW, false);
        draw();
        set_menu(InputDisplay::STATUS, false);
        draw();
    };
    setup();
    prime_history();
    capture("mode_cycle", "smash");
    set_menu(InputDisplay::RAW, false);
    capture("mode_cycle", "raw");
    set_menu(InputDisplay::STATUS, false);
    capture("mode_cycle", "status");
    time_case("mode_cycle", &setup, &body);
}

fn rapid_input() {
    let none = training::input_log::STICK_NONE;
    let setup = || {
        reset_world();
        put_logs(filled(1, none, none));
    };
    setup();
    prime_history();
    // Cross the complete ring twice, with changing buttons and both sticks.
    for index in 0..32 {
        rotate_front(head_log(1 << (index % 8), 600, 1, index, 13, 45, 270));
        capture("rapid_input", &format!("input{index}"));
    }
    time_case("rapid_input_120_frames", &setup, &|| {
        for index in 0..120 {
            rotate_front(head_log(1 << (index % 8), 600, 1, index, 13, 45, 270));
            draw();
        }
    });
}

fn icon_color_and_clear() {
    let none = training::input_log::STICK_NONE;
    reset_world();
    put_logs(filled(0, 90, none));
    prime_history();
    capture("icon_color", "left_white");
    let mut logs = read(&training::input_log::P1_INPUT_LOGS);
    logs[0].lstick = none;
    logs[0].rstick = 90;
    put_logs(logs);
    capture("icon_color", "right_yellow");
    logs[0].rstick = none;
    put_logs(logs);
    capture("icon_color", "cleared");
}

fn capped_frame_count() {
    let none = training::input_log::STICK_NONE;
    reset_world();
    let mut logs = filled(1, none, none);
    logs[0].frames = 99;
    put_logs(logs);
    prime_history();
    capture("frame_cap", "first99");
    logs[0].ttl -= 1;
    put_logs(logs);
    capture("frame_cap", "held99");
}

fn status_toggle() {
    let none = training::input_log::STICK_NONE;
    let setup = || {
        reset_world();
        set_menu(InputDisplay::SMASH, true);
        put_logs(filled(1 << 0, none, none));
    };
    let body = || {
        set_menu(InputDisplay::SMASH, true);
        draw();
        set_menu(InputDisplay::SMASH, false);
        draw();
        let mut logs = read(&training::input_log::P1_INPUT_LOGS);
        logs[0].status = 222;
        logs[0].fighter_kind = 14;
        put_logs(logs);
        set_menu(InputDisplay::SMASH, true);
        draw();
    };
    setup();
    prime_history();
    capture("status_toggle", "on");
    set_menu(InputDisplay::SMASH, false);
    capture("status_toggle", "off");
    let mut logs = read(&training::input_log::P1_INPUT_LOGS);
    logs[0].status = 222;
    logs[0].fighter_kind = 14;
    put_logs(logs);
    set_menu(InputDisplay::SMASH, true);
    capture("status_toggle", "changed");
    time_case("status_toggle", &setup, &body);
}

fn hide_reopen() {
    let none = training::input_log::STICK_NONE;
    let setup = || {
        reset_world();
        set_menu(InputDisplay::SMASH, true);
        put_logs(filled(1 << 0, none, none));
    };
    let body = || {
        draw();
        assign(&menu::QUICK_MENU_ACTIVE, true);
        draw();
        assign(&menu::QUICK_MENU_ACTIVE, false);
        draw();
        assign(&training::ui::menu::VANILLA_MENU_ACTIVE, true);
        let mut logs = read(&training::input_log::P1_INPUT_LOGS);
        logs[0].buttons = 1 << 1;
        logs[0].ttl = 100;
        logs[0].frames = 40;
        logs[0].status = 3;
        put_logs(logs);
        draw();
        assign(&training::ui::menu::VANILLA_MENU_ACTIVE, false);
        draw();
        set_menu(InputDisplay::NONE, true);
        draw();
        set_menu(InputDisplay::SMASH, true);
        draw();
    };
    setup();
    prime_history();
    capture("hide_reopen", "shown");
    assign(&menu::QUICK_MENU_ACTIVE, true);
    capture("hide_reopen", "quick_hidden");
    assign(&menu::QUICK_MENU_ACTIVE, false);
    capture("hide_reopen", "quick_reopen");
    assign(&training::ui::menu::VANILLA_MENU_ACTIVE, true);
    let mut logs = read(&training::input_log::P1_INPUT_LOGS);
    logs[0].buttons = 1 << 1;
    logs[0].ttl = 100;
    logs[0].frames = 40;
    logs[0].status = 3;
    put_logs(logs);
    capture("hide_reopen", "vanilla_hidden");
    assign(&training::ui::menu::VANILLA_MENU_ACTIVE, false);
    capture("hide_reopen", "vanilla_reopen");
    set_menu(InputDisplay::NONE, true);
    capture("hide_reopen", "none_hidden");
    set_menu(InputDisplay::SMASH, true);
    capture("hide_reopen", "smash_reopen");
    time_case("hide_reopen", &setup, &body);
}

fn ttl_fade() {
    let none = training::input_log::STICK_NONE;
    let ttls = [0u32, 1, 50, 100, 199, 200, 201, 300, 400, 500, 600, 0, 10, 20, 199];
    let setup = || {
        reset_world();
        let mut logs = [training::input_log::InputLog::default(); 15];
        for index in 0..15 {
            logs[index] = head_log(
                if index == 0 { 1 << 0 } else { 0 },
                ttls[index],
                1 + index as u32,
                10 + index as i32,
                13,
                none,
                none,
            );
        }
        put_logs(logs);
    };
    setup();
    prime_history();
    capture("ttl_fade", "draw");
    time_case("ttl_fade", &setup, &|| draw());
}

fn decay_logs() {
    let mut logs = read(&training::input_log::P1_INPUT_LOGS);
    for log in logs.iter_mut() {
        if log.ttl > 0 {
            log.ttl -= 1;
        }
    }
    if logs[0].frames < 99 {
        logs[0].frames += 1;
    }
    put_logs(logs);
}

/// Three real ttl decrements after the cache is warm. Rows at or below 200 fade;
/// 201 stays above the fade threshold for the first step.
fn ttl_decay() {
    let none = training::input_log::STICK_NONE;
    let ttls = [500u32, 200, 199, 50, 1, 0, 201, 400, 400, 400, 400, 180, 20, 199, 2];
    let setup = || {
        reset_world();
        let mut logs = [training::input_log::InputLog::default(); 15];
        for index in 0..15 {
            logs[index] = head_log(
                if index == 0 {
                    1 << 0
                } else {
                    1 << ((index % 8) as u32)
                },
                ttls[index],
                40,
                10 + index as i32,
                13,
                none,
                none,
            );
        }
        put_logs(logs);
    };
    let body = || {
        for _ in 0..3 {
            decay_logs();
            draw();
        }
    };
    setup();
    prime_history();
    decay_logs();
    capture("ttl_decay", "frame1");
    decay_logs();
    capture("ttl_decay", "frame2");
    decay_logs();
    capture("ttl_decay", "frame3");
    time_case("ttl_decay", &setup, &body);
}

fn ring_rotate() {
    let none = training::input_log::STICK_NONE;
    let setup = || {
        reset_world();
        put_logs(filled(1 << 0, none, none));
    };
    let body = || {
        draw();
        for turn in 0..3u32 {
            rotate_front(head_log(
                1u32 << (turn + 1),
                480u32 - turn * 10,
                1,
                20 + turn as i32,
                13,
                none,
                none,
            ));
            draw();
        }
    };
    setup();
    prime_history();
    for turn in 0..3u32 {
        rotate_front(head_log(
            1u32 << (turn + 1),
            480u32 - turn * 10,
            1,
            20 + turn as i32,
            13,
            none,
            none,
        ));
        capture("ring_rotate", &format!("turn{}", turn + 1));
    }
    time_case("ring_rotate", &setup, &body);
}

/// Head frame tick while `DRAW_LOG_BASE_IDX` maps the head onto `TrModInputLog14`.
fn late_head_tick() {
    let none = training::input_log::STICK_NONE;
    let setup = || {
        reset_world();
        assign(&training::input_log::DRAW_LOG_BASE_IDX, 1usize);
        put_logs(filled(1 << 0, none, none));
    };
    let body = || {
        draw();
        let mut logs = read(&training::input_log::P1_INPUT_LOGS);
        logs[0].frames = logs[0].frames.saturating_add(1).min(99);
        if logs[0].ttl > 0 {
            logs[0].ttl -= 1;
        }
        put_logs(logs);
        draw();
    };
    setup();
    prime_history();
    capture("late_head", "steady");
    let mut logs = read(&training::input_log::P1_INPUT_LOGS);
    logs[0].frames += 1;
    logs[0].ttl -= 1;
    put_logs(logs);
    capture("late_head", "frame");
    time_case("late_head", &setup, &body);
}

fn reset_logs() {
    let none = training::input_log::STICK_NONE;
    let setup = || {
        reset_world();
        put_logs(filled((1 << 4) | (1 << 5), 45, none));
    };
    let body = || {
        draw();
        put_logs([training::input_log::InputLog::default(); 15]);
        assign(&training::input_log::DRAW_LOG_BASE_IDX, 0usize);
        draw();
    };
    setup();
    prime_history();
    capture("reset", "dirty");
    put_logs([training::input_log::InputLog::default(); 15]);
    assign(&training::input_log::DRAW_LOG_BASE_IDX, 0usize);
    capture("reset", "cleared");
    time_case("reset", &setup, &body);
}

fn reregister() {
    let none = training::input_log::STICK_NONE;
    let logs = filled(1 << 7, none, 270);
    let setup = || {
        reset_world();
        put_logs(logs);
    };
    let body = || {
        draw();
        skyline::nn::ui2d::reset_panes();
        draw();
        invalidate_renderer();
        draw();
    };
    setup();
    prime_history();
    capture("reregister", "established");
    skyline::nn::ui2d::reset_panes();
    capture("reregister", "stale_redraw");
    invalidate_renderer();
    capture("reregister", "after_invalidate");
    time_case("reregister", &setup, &body);
}
