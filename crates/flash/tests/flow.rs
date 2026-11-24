//! The guided flow end to end, with temp directories standing in for bootloader drives.

use std::path::{Path, PathBuf};

use mxkeys_flash::copy::{copy_uf2, CopyOutcome, TARGET_NAME};
use mxkeys_flash::session::{plan, Action, Firmware, Phase, Session, StepState};
use mxkeys_flash::uf2::block;
use mxkeys_hardware::catalog::ControllerId;
use mxkeys_hardware::detect::uf2::parse_info_uf2;
use mxkeys_probe::{DriveScanner, FsDriveScanner, Uf2Drive};

const NRF52840: u32 = 0xADA5_2840;
const NICE_NANO_INFO: &str =
    "UF2 Bootloader 0.6.0 lib/nrfx (v2.0.0)\nModel: nice!nano\nBoard-ID: nRF52840-nicenano\n";

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("mxkeys-flow-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn write_uf2(path: &Path, family: u32, blocks: u32) {
    let mut data = Vec::new();
    for i in 0..blocks {
        data.extend(block(0x26000 + i * 256, family));
    }
    std::fs::write(path, data).unwrap();
}

fn firmware(dir: &Path) -> Firmware {
    let fw = Firmware::in_dir(dir, true);
    let (left, right) = (fw.parts[0].1.clone(), fw.parts[1].1.clone());
    write_uf2(&left, NRF52840, 40);
    write_uf2(&right, NRF52840, 30);
    write_uf2(&fw.settings_reset, NRF52840, 10);
    fw
}

/// A fake bootloader drive: a folder with INFO_UF2.TXT, found by the real scanner.
fn plug_in(root: &Path, info: &str) -> PathBuf {
    let d = root.join("NICENANO");
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("INFO_UF2.TXT"), info).unwrap();
    d
}

fn scan(root: &Path) -> Vec<Uf2Drive> {
    FsDriveScanner::with_roots(vec![root.join("NICENANO")])
        .uf2_drives()
        .unwrap()
}

#[test]
fn full_flow_with_settings_reset() {
    let dir = tmp("full");
    let mounts = tmp("full-mounts");
    let fw = firmware(&dir);
    let (left, right) = (fw.parts[0].1.clone(), fw.parts[1].1.clone());
    let mut s = Session::new(plan(&fw, true), ControllerId::NiceNanoV2).unwrap();
    assert_eq!(s.view().steps.len(), 4);
    assert_eq!(s.view().steps[0].title, "Reset the left half");

    let expected_files = [&fw.settings_reset, &fw.settings_reset, &left, &right];
    for (i, file) in expected_files.iter().enumerate() {
        // Nothing plugged in yet.
        assert_eq!(s.on_drives(&scan(&mounts)), None);
        assert!(
            s.view().instruction.contains("press its reset button twice"),
            "{}",
            s.view().instruction
        );
        // Double-tap: the drive appears → copy.
        let drive = plug_in(&mounts, NICE_NANO_INFO);
        let action = s.on_drives(&scan(&mounts)).expect("copy");
        let Action::Copy { drive: d, file: f } = action;
        assert_eq!((&d, &f), (&drive, *file));
        let outcome = copy_uf2(&f, &d, |_, _| {}).unwrap();
        assert_eq!(
            std::fs::read(drive.join(TARGET_NAME)).unwrap(),
            std::fs::read(file).unwrap()
        );
        s.on_copy_done(Ok(outcome));
        // Still there: restarting.
        s.on_drives(&scan(&mounts));
        assert!(matches!(s.phase(), Phase::Restarting { .. }));
        // The bootloader resets: the drive disappears → next step.
        std::fs::remove_dir_all(&drive).unwrap();
        s.on_drives(&scan(&mounts));
        assert_eq!(s.view().steps[i].state, StepState::Done);
    }
    assert!(s.is_finished());
    assert_eq!(s.view().instruction, "All done!");
}

#[test]
fn refuses_wrong_device_and_recovers() {
    let dir = tmp("refuse");
    let mounts = tmp("refuse-mounts");
    let fw = firmware(&dir);
    let mut s = Session::new(plan(&fw, false), ControllerId::NiceNanoV2).unwrap();
    let rp = plug_in(
        &mounts,
        "UF2 Bootloader v3.0\nModel: Raspberry Pi RP2\nBoard-ID: RPI-RP2\n",
    );
    assert_eq!(s.on_drives(&scan(&mounts)), None);
    let v = s.view();
    assert!(matches!(v.phase, Phase::Refused { .. }));
    assert!(v.detail.unwrap().contains("Nothing was copied"));
    assert!(
        !rp.join(TARGET_NAME).exists(),
        "nothing written to the wrong device"
    );
    // Unplug it → back to waiting; plug in the right one → copy.
    std::fs::remove_dir_all(&rp).unwrap();
    s.on_drives(&scan(&mounts));
    assert_eq!(s.phase(), &Phase::WaitingForDrive);
    plug_in(&mounts, NICE_NANO_INFO);
    assert!(s.on_drives(&scan(&mounts)).is_some());
}

#[test]
fn refuses_firmware_for_another_chip_before_starting() {
    let dir = tmp("chip");
    let fw = firmware(&dir);
    let (left, right) = (fw.parts[0].1.clone(), fw.parts[1].1.clone());
    write_uf2(&right, 0xE48B_FF56, 5); // RP2040 family
    let err = Session::new(plan(&fw, false), ControllerId::NiceNanoV2).unwrap_err();
    assert_eq!(err.0, right);
    assert!(err.1.to_string().contains("isn't for the nice!nano v2"));
    std::fs::write(&left, b"not a uf2").unwrap();
    assert!(Session::new(plan(&fw, false), ControllerId::NiceNanoV2).is_err());
}

#[test]
fn too_many_drives_failure_retry_skip_and_stuck_hint() {
    let dir = tmp("misc");
    let fw = firmware(&dir);
    let mut s = Session::new(plan(&fw, false), ControllerId::NiceNanoV2).unwrap();
    let info = parse_info_uf2(NICE_NANO_INFO);
    let d = |p: &str| Uf2Drive {
        path: PathBuf::from(p),
        info: info.clone(),
    };

    assert_eq!(s.on_drives(&[d("/a"), d("/b")]), None);
    assert_eq!(s.phase(), &Phase::TooManyDrives);
    assert!(s.on_drives(&[d("/a")]).is_some());
    s.on_copy_done(Err("copying stopped after 0 of 20480 bytes".into()));
    assert!(matches!(s.phase(), Phase::Failed { .. }));
    s.retry();
    assert!(s.on_drives(&[d("/a")]).is_some());
    s.on_copy_done(Ok(CopyOutcome::WrittenThenDisconnected));
    for _ in 0..25 {
        s.on_drives(&[d("/a")]);
    }
    assert!(s.view().detail.unwrap().contains("still there"));
    s.on_drives(&[]);
    assert_eq!(s.view().current, Some(1));
    s.skip();
    assert!(s.is_finished());
    assert_eq!(s.view().steps[1].state, StepState::Skipped);
}

#[test]
fn one_piece_keyboard_flashes_once() {
    let dir = tmp("whole");
    let fw = Firmware::in_dir(&dir, false);
    write_uf2(&fw.parts[0].1, NRF52840, 20);
    write_uf2(&fw.settings_reset, NRF52840, 10);
    let s = Session::new(plan(&fw, true), ControllerId::NiceNanoV2).unwrap();
    let v = s.view();
    let titles: Vec<&str> = v.steps.iter().map(|x| x.title.as_str()).collect();
    assert_eq!(titles, ["Reset the keyboard", "Install firmware on the keyboard"]);
    assert!(v
        .instruction
        .starts_with("Plug the keyboard into this computer with USB, then"));
}
