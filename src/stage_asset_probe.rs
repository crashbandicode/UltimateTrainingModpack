//! Opt-in bounded development capture of owned-game stage assets through read-only arc:/.
//! Candidate thumbnails come from form-0 symbols in the native stage table;
//! failed reads are reported, not treated as proof of selectable stages.
//! Never call arcrop_load_file outside its native file callback/lock context.

use skyline::nn::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

static READY: AtomicBool = AtomicBool::new(false);
const OUTPUT: &str = "sd:/ultimate/TrainingModpack/stage-asset-probe";
const PATHS: &[&str] = &[
    "ui/replace/stage/stage_1/stage_1_battlefield.bntx",
    "ui/replace_patch/stage/stage_1/stage_1_battlefields.bntx",
    "ui/replace/stage/stage_1/stage_1_battlefieldl.bntx",
    "ui/replace/stage/stage_1/stage_1_end.bntx",
    "ui/replace/stage/stage_1/stage_1_mario_castle64.bntx",
    "ui/replace/stage/stage_1/stage_1_dk_jungle.bntx",
    "ui/replace/stage/stage_1/stage_1_zelda_hyrule.bntx",
    "ui/replace/stage/stage_1/stage_1_yoshi_story.bntx",
    "ui/replace/stage/stage_1/stage_1_kirby_pupupu64.bntx",
    "ui/replace/stage/stage_1/stage_1_poke_yamabuki.bntx",
    "ui/replace/stage/stage_1/stage_1_mario_past64.bntx",
    "ui/replace/stage/stage_1/stage_1_mario_castledx.bntx",
    "ui/replace/stage/stage_1/stage_1_mario_rainbow.bntx",
    "ui/replace/stage/stage_1/stage_1_dk_waterfall.bntx",
    "ui/replace/stage/stage_1/stage_1_dk_lodge.bntx",
    "ui/replace/stage/stage_1/stage_1_zelda_greatbay.bntx",
    "ui/replace/stage/stage_1/stage_1_zelda_temple.bntx",
    "ui/replace/stage/stage_1/stage_1_metroid_zebesdx.bntx",
    "ui/replace/stage/stage_1/stage_1_yoshi_yoster.bntx",
    "ui/replace/stage/stage_1/stage_1_yoshi_cartboard.bntx",
    "ui/replace/stage/stage_1/stage_1_kirby_fountain.bntx",
    "ui/replace/stage/stage_1/stage_1_kirby_greens.bntx",
    "ui/replace/stage/stage_1/stage_1_fox_corneria.bntx",
    "ui/replace/stage/stage_1/stage_1_fox_venom.bntx",
    "ui/replace/stage/stage_1/stage_1_poke_stadium.bntx",
    "ui/replace/stage/stage_1/stage_1_mother_onett.bntx",
    "ui/replace/stage/stage_1/stage_1_mario_pastusa.bntx",
    "ui/replace/stage/stage_1/stage_1_metroid_kraid.bntx",
    "ui/replace/stage/stage_1/stage_1_fzero_bigblue.bntx",
    "ui/replace/stage/stage_1/stage_1_mother_fourside.bntx",
    "ui/replace/stage/stage_1/stage_1_mario_dolpic.bntx",
    "ui/replace/stage/stage_1/stage_1_mario_pastx.bntx",
    "ui/replace/stage/stage_1/stage_1_kart_circuitx.bntx",
    "ui/replace/stage/stage_1/stage_1_wario_madein.bntx",
    "ui/replace/stage/stage_1/stage_1_zelda_oldin.bntx",
    "ui/replace/stage/stage_1/stage_1_metroid_norfair.bntx",
    "ui/replace/stage/stage_1/stage_1_metroid_orpheon.bntx",
    "ui/replace/stage/stage_1/stage_1_yoshi_island.bntx",
    "ui/replace/stage/stage_1/stage_1_kirby_halberd.bntx",
    "ui/replace/stage/stage_1/stage_1_fox_lylatcruise.bntx",
    "ui/replace/stage/stage_1/stage_1_poke_stadium2.bntx",
    "ui/replace/stage/stage_1/stage_1_fzero_porttown.bntx",
    "ui/replace/stage/stage_1/stage_1_fe_siege.bntx",
    "ui/replace/stage/stage_1/stage_1_pikmin_planet.bntx",
    "ui/replace/stage/stage_1/stage_1_animal_village.bntx",
    "ui/replace/stage/stage_1/stage_1_mother_newpork.bntx",
    "ui/replace/stage/stage_1/stage_1_ice_top.bntx",
    "ui/replace/stage/stage_1/stage_1_icarus_skyworld.bntx",
    "ui/replace/stage/stage_1/stage_1_mg_shadowmoses.bntx",
    "ui/replace/stage/stage_1/stage_1_luigimansion.bntx",
    "ui/replace/stage/stage_1/stage_1_zelda_pirates.bntx",
    "ui/replace/stage/stage_1/stage_1_poke_tengam.bntx",
    "ui/replace/stage/stage_1/stage_1_75m.bntx",
    "ui/replace/stage/stage_1/stage_1_mariobros.bntx",
    "ui/replace/stage/stage_1/stage_1_plankton.bntx",
    "ui/replace/stage/stage_1/stage_1_sonic_greenhill.bntx",
    "ui/replace/stage/stage_1/stage_1_mario_3dland.bntx",
    "ui/replace/stage/stage_1/stage_1_mario_newbros2.bntx",
    "ui/replace/stage/stage_1/stage_1_mario_paper.bntx",
    "ui/replace/stage/stage_1/stage_1_zelda_gerudo.bntx",
    "ui/replace/stage/stage_1/stage_1_zelda_train.bntx",
    "ui/replace/stage/stage_1/stage_1_kirby_gameboy.bntx",
    "ui/replace/stage/stage_1/stage_1_poke_unova.bntx",
    "ui/replace/stage/stage_1/stage_1_poke_tower.bntx",
    "ui/replace/stage/stage_1/stage_1_fzero_mutecity3ds.bntx",
    "ui/replace/stage/stage_1/stage_1_mother_magicant.bntx",
    "ui/replace/stage/stage_1/stage_1_fe_arena.bntx",
    "ui/replace/stage/stage_1/stage_1_icarus_uprising.bntx",
    "ui/replace/stage/stage_1/stage_1_animal_island.bntx",
    "ui/replace/stage/stage_1/stage_1_balloonfight.bntx",
    "ui/replace/stage/stage_1/stage_1_nintendogs.bntx",
    "ui/replace/stage/stage_1/stage_1_streetpass.bntx",
    "ui/replace/stage/stage_1/stage_1_tomodachi.bntx",
    "ui/replace/stage/stage_1/stage_1_pictochat2.bntx",
    "ui/replace/stage/stage_1/stage_1_mario_uworld.bntx",
    "ui/replace/stage/stage_1/stage_1_mario_galaxy.bntx",
    "ui/replace/stage/stage_1/stage_1_kart_circuitfor.bntx",
    "ui/replace/stage/stage_1/stage_1_zelda_skyward.bntx",
    "ui/replace/stage/stage_1/stage_1_kirby_cave.bntx",
    "ui/replace/stage/stage_1/stage_1_poke_kalos.bntx",
    "ui/replace/stage/stage_1/stage_1_fe_colloseum.bntx",
    "ui/replace/stage/stage_1/stage_1_flatzonex.bntx",
    "ui/replace/stage/stage_1/stage_1_icarus_angeland.bntx",
    "ui/replace/stage/stage_1/stage_1_wario_gamer.bntx",
    "ui/replace/stage/stage_1/stage_1_pikmin_garden.bntx",
    "ui/replace/stage/stage_1/stage_1_animal_city.bntx",
    "ui/replace/stage/stage_1/stage_1_wiifit.bntx",
    "ui/replace/stage/stage_1/stage_1_punchoutsb.bntx",
    "ui/replace/stage/stage_1/stage_1_xeno_gaur.bntx",
    "ui/replace/stage/stage_1/stage_1_duckhunt.bntx",
    "ui/replace/stage/stage_1/stage_1_wreckingcrew.bntx",
    "ui/replace/stage/stage_1/stage_1_pilotwings.bntx",
    "ui/replace/stage/stage_1/stage_1_wufuisland.bntx",
    "ui/replace/stage/stage_1/stage_1_sonic_windyhill.bntx",
    "ui/replace/stage/stage_1/stage_1_rock_wily.bntx",
    "ui/replace/stage/stage_1/stage_1_pac_land.bntx",
    "ui/replace/stage/stage_1/stage_1_mario_maker.bntx",
    "ui/replace/stage/stage_1/stage_1_sf_suzaku.bntx",
    "ui/replace/stage/stage_1/stage_1_ff_midgar.bntx",
    "ui/replace/stage/stage_1/stage_1_bayo_clock.bntx",
    "ui/replace/stage/stage_1/stage_1_mario_odyssey.bntx",
    "ui/replace/stage/stage_1/stage_1_zelda_tower.bntx",
    "ui/replace/stage/stage_1/stage_1_spla_parking.bntx",
    "ui/replace/stage/stage_1/stage_1_dracula_castle.bntx",
    "ui/replace_patch/stage/stage_1/stage_1_jack_mementoes.bntx",
    "ui/replace_patch/stage/stage_1/stage_1_brave_altar.bntx",
    "ui/replace_patch/stage/stage_1/stage_1_buddy_spiral.bntx",
    "ui/replace_patch/stage/stage_1/stage_1_dolly_stadium.bntx",
    "ui/replace_patch/stage/stage_1/stage_1_fe_shrine.bntx",
    "ui/replace_patch/stage/stage_1/stage_1_tantan_spring.bntx",
    "ui/replace_patch/stage/stage_1/stage_1_pickel_world.bntx",
    "ui/replace_patch/stage/stage_1/stage_1_ff_cave.bntx",
    "ui/replace_patch/stage/stage_1/stage_1_xeno_alst.bntx",
    "ui/replace_patch/stage/stage_1/stage_1_demon_dojo.bntx",
    "ui/replace_patch/stage/stage_1/stage_1_trail_castle.bntx",
    "ui/replace/stage/stage_2/stage_2_battlefield.bntx",
    "ui/replace/stage/stage_2/stage_2_end.bntx",
    "ui/replace/stage/stage_3/stage_3_battlefield.bntx",
    "ui/replace/stage/stage_3/stage_3_end.bntx",
];

// ARCropolis public Event is repr(C): ArcFilesystemMounted=0,
// ModFilesystemMounted=1. Its event worker calls this after releasing FS locks.
extern "C" fn mounted(event: u32) {
    if event == 1 {
        READY.store(true, Ordering::Release);
    }
}

struct NativeFile(fs::FileHandle);

impl Drop for NativeFile {
    fn drop(&mut self) {
        unsafe {
            fs::CloseFile(self.0);
        }
    }
}

fn checked(operation: &str, result: u32) -> std::io::Result<()> {
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "{operation}: result=0x{result:x}"
        )))
    }
}

fn capture(path: &str) -> std::io::Result<usize> {
    let name = std::ffi::CString::new(format!("arc:/{path}")).expect("fixed asset path");
    let mut handle = fs::FileHandle { handle: 0 };
    unsafe {
        checked(
            "OpenFile",
            fs::OpenFile(
                &mut handle,
                name.as_ptr().cast(),
                fs::OpenMode_OpenMode_Read as i32,
            ),
        )?;
    }
    let source = NativeFile(handle);
    let mut native_length = 0i64;
    unsafe {
        checked("GetFileSize", fs::GetFileSize(&mut native_length, source.0))?;
    }
    let length = native_length as u64;
    let (cap, magic): (u64, &[u8]) = if path.ends_with(".bntx") {
        (1024 * 1024, b"BNTX\0\0\0\0")
    } else if path.ends_with(".prc") {
        (4 * 1024 * 1024, b"paracobn")
    } else {
        (4 * 1024 * 1024, b"MsgStdBn")
    };
    if native_length <= 0 || length > cap {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "asset size outside probe bound",
        ));
    }
    let mut bytes = vec![0; length as usize];
    let mut actual = 0u64;
    unsafe {
        checked(
            "ReadFile",
            fs::ReadFile1(&mut actual, source.0, 0, bytes.as_mut_ptr(), length),
        )?;
    }
    if actual != length {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            format!("short asset: {actual}/{length}"),
        ));
    }
    if !bytes.starts_with(magic) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "unexpected asset magic",
        ));
    }
    let name = path.rsplit('/').next().expect("fixed asset path");
    crate::logging::diagnostic(format_args!(
        "[StageAssetProbe] read {path} bytes={actual} magic_ok=true"
    ));
    let mut target = std::fs::OpenOptions::new()
        .write(true)
        // Pinned Skyline std requires create as well as create_new: otherwise
        // its GetEntryType precheck returns 514 for a new output filename.
        .create(true)
        .create_new(true)
        .open(format!("{OUTPUT}/{name}"))?;
    std::io::Write::write_all(&mut target, &bytes)?;
    Ok(bytes.len())
}

pub fn init() {
    let mut register = 0usize;
    let mut version = 0usize;
    unsafe {
        skyline::nn::ro::LookupSymbol(&mut register, b"arcrop_register_event_callback\0".as_ptr());
        skyline::nn::ro::LookupSymbol(&mut version, b"arcrop_api_version\0".as_ptr());
    }
    if register == 0 || version == 0 {
        return;
    }
    type Version = unsafe extern "C" fn() -> *const [u32; 2];
    let version = unsafe {
        std::mem::transmute::<usize, Version>(version)()
            .as_ref()
            .copied()
    };
    crate::logging::diagnostic(format_args!("[StageAssetProbe] API={version:?}"));
    if !matches!(version, Some([1, minor]) if minor >= 9) {
        return;
    }
    type Register = unsafe extern "C" fn(u32, extern "C" fn(u32));
    unsafe {
        std::mem::transmute::<usize, Register>(register)(1, mounted);
    }
    let spawned = std::thread::Builder::new().stack_size(0x20000).spawn(|| {
        for _ in 0..120 {
            if READY.load(Ordering::Acquire) {
                break;
            }
            std::thread::sleep(Duration::from_secs(1));
        }
        if !READY.load(Ordering::Acquire) {
            crate::logging::diagnostic(format_args!("[StageAssetProbe] mount event timed out"));
            return;
        }
        if let Err(error) = std::fs::create_dir(OUTPUT) {
            crate::logging::diagnostic(format_args!("[StageAssetProbe] output: {error}"));
            return;
        }
        for path in PATHS {
            crate::logging::diagnostic(format_args!(
                "[StageAssetProbe] {path}: {:?}",
                capture(path)
            ));
        }
    });
    crate::logging::diagnostic(format_args!(
        "[StageAssetProbe] worker_started={}",
        spawned.is_ok()
    ));
}
