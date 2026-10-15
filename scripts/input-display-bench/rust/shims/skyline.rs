//! Host pane tree for the input-log renderer.
//!
//! `find_pane_by_name_recursive` is a depth-first search that checks the pane
//! itself, then each child in order, and stops at the first match. That is the
//! shape of the game's recursive name lookup. Visit and call counters measure
//! that search. This is not the Switch layout library and it is not FPS.

use std::sync::atomic::{AtomicU64, Ordering};

pub mod nn {
    pub mod ui2d {
        use super::super::{
            COLOR_WRITES, LOOKUP_CALLS, LOOKUP_VISITS, TEXT_WRITES, VISIBILITY_WRITES,
        };
        use std::sync::atomic::Ordering;

        pub const ICON_NAMES: [&str; 37] = [
            "a",
            "b",
            "x",
            "y",
            "lb",
            "rb",
            "zl",
            "zr",
            "up",
            "down",
            "left",
            "right",
            "up_left",
            "up_right",
            "down_left",
            "down_right",
            "gcc_l",
            "gcc_r",
            "gcc_z",
            "plus",
            "minus",
            "l_stick",
            "r_stick",
            "gcc_c_stick",
            "dpad_up",
            "dpad_down",
            "dpad_left",
            "dpad_right",
            "dpad_left_right",
            "up_strong",
            "down_strong",
            "left_strong",
            "right_strong",
            "up_left_strong",
            "down_left_strong",
            "up_right_strong",
            "down_right_strong",
        ];

        #[repr(C)]
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct ResColor {
            pub r: u8,
            pub g: u8,
            pub b: u8,
            pub a: u8,
        }

        #[repr(C)]
        #[derive(Clone, Copy, Debug)]
        pub enum PaneFlag {
            Visible,
            InfluencedAlpha,
            LocationAdjust,
            UserAllocated,
            IsGlobalMatrixDirty,
            UserMatrix,
            UserGlobalMatrix,
            IsConstantBufferReady,
            Max,
        }

        #[repr(C)]
        pub struct Material {
            pub black: ResColor,
        }

        impl Material {
            pub fn set_black_res_color(&mut self, black: ResColor) {
                COLOR_WRITES.fetch_add(1, Ordering::Relaxed);
                self.black = black;
            }
        }

        #[repr(C)]
        pub struct Pane {
            pub pos_y: f32,
            pub flags: u8,
            pub alpha: u8,
            pub global_alpha: u8,
            id: u32,
        }

        #[repr(C)]
        pub struct Picture {
            pub pane: Pane,
            pub material: *mut Material,
        }

        #[repr(C)]
        pub struct TextBox {
            pub pane: Pane,
            len: u8,
            text: [u8; 95],
        }

        impl std::ops::Deref for Picture {
            type Target = Pane;
            fn deref(&self) -> &Pane {
                &self.pane
            }
        }

        impl std::ops::DerefMut for Picture {
            fn deref_mut(&mut self) -> &mut Pane {
                &mut self.pane
            }
        }

        impl TextBox {
            pub fn write_text(&mut self, text: &str) {
                TEXT_WRITES.fetch_add(1, Ordering::Relaxed);
                let bytes = text.as_bytes();
                assert!(bytes.len() <= self.text.len(), "host text buffer overflow");
                self.text[..bytes.len()].copy_from_slice(bytes);
                self.len = bytes.len() as u8;
            }

            fn as_str(&self) -> String {
                String::from_utf8_lossy(&self.text[..self.len as usize]).into_owned()
            }
        }

        impl Pane {
            pub unsafe fn as_picture(&mut self) -> &mut Picture {
                &mut *(self as *mut Pane as *mut Picture)
            }

            pub unsafe fn as_textbox(&mut self) -> &mut TextBox {
                &mut *(self as *mut Pane as *mut TextBox)
            }

            pub unsafe fn set_visible(&mut self, visible: bool) {
                VISIBILITY_WRITES.fetch_add(1, Ordering::Relaxed);
                if visible {
                    self.flags |= 1 << PaneFlag::Visible as u8;
                } else {
                    self.flags &= !(1 << PaneFlag::Visible as u8);
                }
            }

            /// One Rust lookup. The recursive walk counts a visit per pane examined.
            pub unsafe fn find_pane_by_name_recursive(&self, name: &str) -> Option<&mut Pane> {
                LOOKUP_CALLS.fetch_add(1, Ordering::Relaxed);
                let found = walk(self.id, name)?;
                Some(&mut *pane_ptr(found))
            }
        }

        struct NodeMeta {
            name: &'static str,
            child_start: u32,
            child_len: u16,
            kind: u8,
        }

        const KIND_PLAIN: u8 = 0;
        const KIND_PICTURE: u8 = 1;
        const KIND_TEXT: u8 = 2;

        struct Forest {
            plain: Vec<Pane>,
            picture: Vec<Picture>,
            text: Vec<TextBox>,
            materials: Vec<Material>,
            meta: Vec<NodeMeta>,
            children: Vec<u32>,
            slot_of: Vec<u32>,
            logs: u32,
            rows: [RowRef; 15],
        }

        #[derive(Clone, Copy)]
        struct RowRef {
            row: u32,
            frame: u32,
            status: u32,
            slots: [u32; 5],
            icons: [[u32; 37]; 5],
        }

        const EMPTY_ROW: RowRef = RowRef {
            row: 0,
            frame: 0,
            status: 0,
            slots: [0; 5],
            icons: [[0; 37]; 5],
        };

        static mut FOREST: Option<Forest> = None;

        fn forest_ptr() -> *mut Forest {
            unsafe {
                (*std::ptr::addr_of_mut!(FOREST))
                    .as_mut()
                    .expect("pane tree built")
            }
        }

        fn pane_ptr(id: u32) -> *mut Pane {
            let tree = unsafe { &mut *forest_ptr() };
            let slot = tree.slot_of[id as usize] as usize;
            match tree.meta[id as usize].kind {
                KIND_PLAIN => &mut tree.plain[slot] as *mut Pane,
                KIND_PICTURE => &mut tree.picture[slot].pane as *mut Pane,
                KIND_TEXT => &mut tree.text[slot].pane as *mut Pane,
                _ => unreachable!("pane kind"),
            }
        }

        fn walk(id: u32, name: &str) -> Option<u32> {
            LOOKUP_VISITS.fetch_add(1, Ordering::Relaxed);
            let mut kids = [0u32; 40];
            let len = unsafe {
                let tree = &*forest_ptr();
                let meta = &tree.meta[id as usize];
                if meta.name == name {
                    return Some(id);
                }
                let len = meta.child_len as usize;
                let start = meta.child_start as usize;
                assert!(len <= kids.len(), "pane has more children than the host walker");
                kids[..len].copy_from_slice(&tree.children[start..start + len]);
                len
            };
            for index in 0..len {
                if let Some(found) = walk(kids[index], name) {
                    return Some(found);
                }
            }
            None
        }

        pub fn ensure_tree() {
            unsafe {
                if (*std::ptr::addr_of!(FOREST)).is_some() {
                    return;
                }
            }
            let mut plain = Vec::new();
            let mut picture = Vec::new();
            let mut text = Vec::new();
            let mut materials = Vec::new();
            let mut meta = Vec::new();
            let mut children = Vec::new();
            let mut slot_of = Vec::new();

            fn add_meta(
                meta: &mut Vec<NodeMeta>,
                slot_of: &mut Vec<u32>,
                name: &'static str,
                kind: u8,
                slot: u32,
            ) -> u32 {
                let id = meta.len() as u32;
                meta.push(NodeMeta {
                    name,
                    child_start: 0,
                    child_len: 0,
                    kind,
                });
                slot_of.push(slot);
                id
            }

            let root_slot = plain.len() as u32;
            plain.push(initial_pane(0, true));
            let root = add_meta(&mut meta, &mut slot_of, "Root", KIND_PLAIN, root_slot);

            let logs_slot = plain.len() as u32;
            plain.push(initial_pane(0, true));
            let logs = add_meta(
                &mut meta,
                &mut slot_of,
                "TrModInputLog",
                KIND_PLAIN,
                logs_slot,
            );

            let mut rows = [EMPTY_ROW; 15];
            for row_index in 0..15 {
                let row_name: &'static str = match row_index {
                    0 => "TrModInputLog0",
                    1 => "TrModInputLog1",
                    2 => "TrModInputLog2",
                    3 => "TrModInputLog3",
                    4 => "TrModInputLog4",
                    5 => "TrModInputLog5",
                    6 => "TrModInputLog6",
                    7 => "TrModInputLog7",
                    8 => "TrModInputLog8",
                    9 => "TrModInputLog9",
                    10 => "TrModInputLog10",
                    11 => "TrModInputLog11",
                    12 => "TrModInputLog12",
                    13 => "TrModInputLog13",
                    14 => "TrModInputLog14",
                    _ => unreachable!("row"),
                };
                let row_slot = plain.len() as u32;
                plain.push(initial_pane(0, false));
                let row = add_meta(&mut meta, &mut slot_of, row_name, KIND_PLAIN, row_slot);
                let frame_slot = text.len() as u32;
                text.push(TextBox {
                    pane: initial_pane(0, true),
                    len: 0,
                    text: [0; 95],
                });
                let frame = add_meta(&mut meta, &mut slot_of, "Frame", KIND_TEXT, frame_slot);
                let status_slot = text.len() as u32;
                text.push(TextBox {
                    pane: initial_pane(0, true),
                    len: 0,
                    text: [0; 95],
                });
                let status = add_meta(&mut meta, &mut slot_of, "Status", KIND_TEXT, status_slot);
                let mut slots = [0; 5];
                let mut icons = [[0; 37]; 5];
                for slot_index in 0..5 {
                    let slot_name: &'static str = match slot_index {
                        0 => "Input0",
                        1 => "Input1",
                        2 => "Input2",
                        3 => "Input3",
                        4 => "Input4",
                        _ => unreachable!("slot"),
                    };
                    let plain_slot = plain.len() as u32;
                    plain.push(initial_pane(0, true));
                    let slot = add_meta(&mut meta, &mut slot_of, slot_name, KIND_PLAIN, plain_slot);
                    slots[slot_index] = slot;
                    for (icon_index, icon_name) in ICON_NAMES.iter().copied().enumerate() {
                        let material_index = materials.len();
                        materials.push(Material {
                            black: ResColor {
                                r: 0,
                                g: 0,
                                b: 0,
                                a: 0,
                            },
                        });
                        let picture_slot = picture.len() as u32;
                        picture.push(Picture {
                            pane: initial_pane(0, true),
                            material: std::ptr::null_mut(),
                        });
                        let icon = add_meta(
                            &mut meta,
                            &mut slot_of,
                            icon_name,
                            KIND_PICTURE,
                            picture_slot,
                        );
                        icons[slot_index][icon_index] = icon;
                        let _ = material_index;
                    }
                }
                rows[row_index] = RowRef {
                    row,
                    frame,
                    status,
                    slots,
                    icons,
                };
            }

            link(&mut meta, &mut children, root, &[logs]);
            let row_ids: Vec<u32> = rows.iter().map(|row| row.row).collect();
            link(&mut meta, &mut children, logs, &row_ids);
            for row in &rows {
                let mut kids = vec![row.frame, row.status];
                kids.extend_from_slice(&row.slots);
                link(&mut meta, &mut children, row.row, &kids);
                for slot_index in 0..5 {
                    link(
                        &mut meta,
                        &mut children,
                        row.slots[slot_index],
                        &row.icons[slot_index],
                    );
                }
            }

            // Fix pane ids now that slots are stable, then point materials at the leaked vec.
            assign_ids(&mut plain, &mut picture, &mut text, &meta, &slot_of);
            let mut tree = Forest {
                plain,
                picture,
                text,
                materials,
                meta,
                children,
                slot_of,
                logs,
                rows,
            };
            bind_materials(&mut tree);
            unsafe {
                *std::ptr::addr_of_mut!(FOREST) = Some(tree);
            }
        }

        fn assign_ids(
            plain: &mut [Pane],
            picture: &mut [Picture],
            text: &mut [TextBox],
            meta: &[NodeMeta],
            slot_of: &[u32],
        ) {
            for (id, node) in meta.iter().enumerate() {
                let slot = slot_of[id] as usize;
                let pane = match node.kind {
                    KIND_PLAIN => &mut plain[slot],
                    KIND_PICTURE => &mut picture[slot].pane,
                    KIND_TEXT => &mut text[slot].pane,
                    _ => unreachable!("pane kind"),
                };
                pane.id = id as u32;
            }
        }

        fn bind_materials(tree: &mut Forest) {
            let base = tree.materials.as_mut_ptr();
            for (index, picture) in tree.picture.iter_mut().enumerate() {
                picture.material = unsafe { base.add(index) };
            }
        }

        fn link(meta: &mut [NodeMeta], children: &mut Vec<u32>, parent: u32, kids: &[u32]) {
            let start = children.len() as u32;
            children.extend_from_slice(kids);
            meta[parent as usize].child_start = start;
            meta[parent as usize].child_len = kids.len() as u16;
        }

        fn initial_pane(id: u32, visible: bool) -> Pane {
            Pane {
                pos_y: 0.0,
                flags: if visible { 1 << PaneFlag::Visible as u8 } else { 0 },
                alpha: 255,
                global_alpha: 255,
                id,
            }
        }

        pub fn reset_panes() {
            unsafe {
                let tree = &mut *forest_ptr();
                let plain_len = tree.plain.len();
                for index in 0..plain_len {
                    let id = tree.plain[index].id;
                    let name = tree.meta[id as usize].name;
                    let visible = id == 0 || name == "TrModInputLog" || name.starts_with("Input");
                    tree.plain[index] = initial_pane(id, visible);
                }
                for picture in &mut tree.picture {
                    let id = picture.pane.id;
                    picture.pane = initial_pane(id, true);
                }
                for text in &mut tree.text {
                    let id = text.pane.id;
                    text.pane = initial_pane(id, true);
                    text.len = 0;
                }
                for material in &mut tree.materials {
                    material.black = ResColor {
                        r: 0,
                        g: 0,
                        b: 0,
                        a: 0,
                    };
                }
            }
        }

        pub fn root_ptr() -> *mut Pane {
            ensure_tree();
            pane_ptr(0)
        }

        pub fn full_state() -> String {
            let (logs_id, rows) = unsafe {
                let tree = &*forest_ptr();
                (tree.logs, tree.rows)
            };
            let mut out = String::new();
            let logs = unsafe { &*pane_ptr(logs_id) };
            out.push_str(&format!("logs visible={}\n", visible_bit(logs.flags)));
            for (index, row) in rows.iter().enumerate() {
                let pane = unsafe { &*pane_ptr(row.row) };
                out.push_str(&format!(
                    "row {index} visible={} pos_bits={} alpha={} global={} flags={}\n",
                    visible_bit(pane.flags),
                    pane.pos_y.to_bits(),
                    pane.alpha,
                    pane.global_alpha,
                    pane.flags
                ));
                out.push_str(&format!(" frame={}\n", text_of(row.frame)));
                out.push_str(&format!(" status={}\n", text_of(row.status)));
                for slot_index in 0..5 {
                    for icon_index in 0..37 {
                        let icon = row.icons[slot_index][icon_index];
                        let name = unsafe { &mut *forest_ptr() }.meta[icon as usize].name;
                        let picture = unsafe { &*pane_ptr(icon).cast::<Picture>() };
                        let color = unsafe { (&*picture.material).black };
                        out.push_str(&format!(
                            " slot {slot_index} {name} visible={} black={},{},{},{}\n",
                            visible_bit(picture.pane.flags),
                            color.r,
                            color.g,
                            color.b,
                            color.a
                        ));
                    }
                }
            }
            out
        }

        pub fn visible_state() -> String {
            let logs_id = unsafe { (&*forest_ptr()).logs };
            let logs = unsafe { &*pane_ptr(logs_id) };
            if logs.flags & (1 << PaneFlag::Visible as u8) == 0 {
                return "hidden\n".to_string();
            }
            full_state()
        }

        fn text_of(id: u32) -> String {
            let text = unsafe { &*pane_ptr(id).cast::<TextBox>() };
            text.as_str()
        }

        fn visible_bit(flags: u8) -> u8 {
            if flags & (1 << PaneFlag::Visible as u8) == 0 {
                0
            } else {
                1
            }
        }

        pub fn node_count() -> usize {
            unsafe { &mut *forest_ptr() }.meta.len()
        }
    }
}

static LOOKUP_CALLS: AtomicU64 = AtomicU64::new(0);
static LOOKUP_VISITS: AtomicU64 = AtomicU64::new(0);
static VISIBILITY_WRITES: AtomicU64 = AtomicU64::new(0);
static TEXT_WRITES: AtomicU64 = AtomicU64::new(0);
static COLOR_WRITES: AtomicU64 = AtomicU64::new(0);
static FADE_WRITES: AtomicU64 = AtomicU64::new(0);

pub struct Counters {
    pub lookup_calls: u64,
    pub lookup_visits: u64,
    pub visibility_writes: u64,
    pub text_writes: u64,
    pub color_writes: u64,
    pub fade_writes: u64,
}

pub fn reset_counters() {
    LOOKUP_CALLS.store(0, Ordering::Relaxed);
    LOOKUP_VISITS.store(0, Ordering::Relaxed);
    VISIBILITY_WRITES.store(0, Ordering::Relaxed);
    TEXT_WRITES.store(0, Ordering::Relaxed);
    COLOR_WRITES.store(0, Ordering::Relaxed);
    FADE_WRITES.store(0, Ordering::Relaxed);
}

pub fn counters() -> Counters {
    Counters {
        lookup_calls: LOOKUP_CALLS.load(Ordering::Relaxed),
        lookup_visits: LOOKUP_VISITS.load(Ordering::Relaxed),
        visibility_writes: VISIBILITY_WRITES.load(Ordering::Relaxed),
        text_writes: TEXT_WRITES.load(Ordering::Relaxed),
        color_writes: COLOR_WRITES.load(Ordering::Relaxed),
        fade_writes: FADE_WRITES.load(Ordering::Relaxed),
    }
}

pub fn note_fade() {
    FADE_WRITES.fetch_add(1, Ordering::Relaxed);
}
