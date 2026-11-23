//! End-to-end detection scenarios on fixture inputs.

use std::path::PathBuf;

use mxkeys_hardware::catalog::{ControllerId, DisplayKind, KeyboardId, Side};
use mxkeys_hardware::detect::quiz::QuizAnswers;
use mxkeys_hardware::detect::uf2::{parse_info_uf2, Uf2Info};
use mxkeys_hardware::detect::usb::UsbDevice;
use mxkeys_hardware::detect::{detect, Confidence, Detection, Inputs, Known, NoteLevel, Question, Source};
use mxkeys_hardware::HardwareProfile;
use zmk_config::build_yaml::BuildMatrix;
use zmk_config::kconfig::KconfigFile;

fn fixture(name: &str) -> String {
    std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .unwrap()
}
fn build(name: &str) -> BuildMatrix {
    BuildMatrix::parse(&fixture(name)).unwrap()
}
fn conf(name: &str) -> KconfigFile {
    KconfigFile::parse(fixture(name))
}
fn uf2(name: &str) -> Uf2Info {
    parse_info_uf2(&fixture(&format!("uf2/{name}")))
}
fn usb(name: &str) -> Vec<UsbDevice> {
    serde_json::from_str(&fixture(&format!("usb/{name}"))).unwrap()
}

fn owner_repo_detection(drives: &[Uf2Info], usb: &[UsbDevice], quiz: Option<&QuizAnswers>) -> Detection {
    let b = build("owner_corne_build.yaml");
    let c = [conf("owner_corne.conf")];
    detect(&Inputs {
        build: Some(&b),
        confs: &c,
        bootloader_drives: drives,
        usb,
        quiz,
    })
}

#[test]
fn owners_repo_alone() {
    let d = owner_repo_detection(&[], &[], None);
    assert_eq!(d.keyboard.value, Some(Known::Known(KeyboardId::Corne)));
    assert_eq!(d.controller.value, Some(Known::Known(ControllerId::NiceNanoV2)));
    assert_eq!(d.controller.confidence, Some(Confidence::Medium));
    assert_eq!(d.display.value, Some(DisplayKind::NiceView));
    assert_eq!(d.central.value, Some(Side::Left));
    // build.yaml snippet + CONFIG_ZMK_STUDIO=y agree, but both are config → medium.
    assert_eq!(d.studio.value, Some(true));
    assert_eq!(d.studio.confidence, Some(Confidence::Medium));
    assert_eq!(
        d.summary,
        "Corne · nice!nano v2 · nice!view — detected from build.yaml"
    );
    assert_eq!(d.profile, Some(HardwareProfile::CORNE_NICE_NANO_V2_NICE_VIEW));
    assert!(d.ask.is_empty(), "{:?}", d.ask);
    assert!(d.keyboard.conflicts.is_empty() && d.display.conflicts.is_empty());
    // The screen is switched off in the owner's settings — must be pointed out.
    let warning = d
        .notes
        .iter()
        .find(|n| n.level == NoteLevel::Warning)
        .expect("screen-off warning");
    assert!(
        warning.text.contains("CONFIG_ZMK_DISPLAY=n") && warning.text.contains("nice!view"),
        "{}",
        warning.text
    );
    assert!(d
        .notes
        .iter()
        .any(|n| n.text.contains("with and without the screen")));
}

#[test]
fn bootloader_confirms_controller() {
    let d = owner_repo_detection(&[uf2("nice_nano_v2.txt")], &[], None);
    assert_eq!(d.controller.value, Some(Known::Known(ControllerId::NiceNanoV2)));
    assert_eq!(d.controller.confidence, Some(Confidence::Strong));
    assert_eq!(d.controller.sources, [Source::BuildYaml, Source::Bootloader]);
    assert_eq!(
        d.summary,
        "Corne · nice!nano v2 · nice!view — detected from build.yaml + bootloader"
    );
    assert!(d.notes.iter().any(|n| n.text == "Bootloader version 0.6.0."));
}

#[test]
fn bootloader_disagrees_with_build_yaml() {
    let d = owner_repo_detection(&[uf2("rp2040.txt")], &[], None);
    assert_eq!(d.controller.value, Some(Known::Other("Raspberry Pi RP2".into())));
    assert_eq!(d.controller.confidence, Some(Confidence::Strong));
    assert_eq!(
        d.controller.conflicts,
        ["According to your build.yaml, the controller is a nice!nano v2, but according to the bootloader drive, the controller is a “Raspberry Pi RP2”. The keyboard itself reports this, so we trust it."]
    );
    assert_eq!(d.profile, None, "unknown controller → no complete profile");
    assert!(d.ask.contains(&Question::Controller));
}

#[test]
fn user_says_there_is_a_screen_but_config_has_none() {
    let b = BuildMatrix::parse(
        "include:\n  - board: nice_nano@2.0.0\n    shield: corne_left\n  - board: nice_nano@2.0.0\n    shield: corne_right\n",
    )
    .unwrap();
    let quiz = QuizAnswers {
        display: Some(DisplayKind::NiceView),
        ..Default::default()
    };
    let d = detect(&Inputs {
        build: Some(&b),
        quiz: Some(&quiz),
        ..Default::default()
    });
    assert_eq!(d.display.value, Some(DisplayKind::NiceView));
    assert_eq!(
        d.display.conflicts,
        ["Your config doesn't build firmware for a screen yet, but according to your answers, you have a nice!view. The app will build with the nice!view so the screen works."]
    );
    assert_eq!(
        d.profile.unwrap().build_targets()[1].shields,
        ["corne_right", "nice_view_adapter", "nice_view"]
    );
    // Nothing turns live editing on.
    assert_eq!(d.studio.value, Some(false));
    assert!(d.notes.iter().any(|n| n.text.contains("settings reset")));
}

#[test]
fn legacy_v0_3_repo() {
    let b = build("owner_macos_build.yaml");
    let c = [conf("owner_macos.conf")];
    let d = detect(&Inputs {
        build: Some(&b),
        confs: &c,
        ..Default::default()
    });
    assert_eq!(d.profile, Some(HardwareProfile::CORNE_NICE_NANO_V2_NICE_VIEW));
    assert!(d.notes.iter().any(|n| n.text.contains("nice_nano_v2")));
    assert!(
        !d.notes.iter().any(|n| n.level == NoteLevel::Warning),
        "screen not switched off here: {:?}",
        d.notes
    );
}

#[test]
fn other_controllers_and_keyboards() {
    for (file, id) in [
        ("xiao_ble.txt", ControllerId::XiaoBle),
        ("nrfmicro.txt", ControllerId::NrfMicro),
    ] {
        let d = detect(&Inputs {
            bootloader_drives: &[uf2(file)],
            ..Default::default()
        });
        assert_eq!(d.controller.value, Some(Known::Known(id)), "{file}");
    }
    assert!(detect(&Inputs {
        bootloader_drives: &[uf2("garbage.txt")],
        ..Default::default()
    })
    .controller
    .value
    .is_none());

    let b = BuildMatrix::parse("include:\n  - board: xiao_ble\n    shield: lily58_left\n").unwrap();
    let d = detect(&Inputs {
        build: Some(&b),
        ..Default::default()
    });
    assert_eq!(d.keyboard.value, Some(Known::Other("lily58".into())));
    assert_eq!(d.controller.value, Some(Known::Known(ControllerId::XiaoBle)));
    assert_eq!(d.profile, None);

    let b = BuildMatrix::parse("include:\n  - board: xiao_ble\n    shield: corne_left\n").unwrap();
    let p = detect(&Inputs {
        build: Some(&b),
        ..Default::default()
    })
    .profile
    .unwrap();
    assert_eq!(
        p.unsupported_parts(),
        ["the Seeed XIAO nRF52840 (XIAO BLE) controller"]
    );
}

#[test]
fn nothing_known() {
    let d = detect(&Inputs::default());
    assert_eq!(
        d.summary,
        "Keyboard unknown · controller unknown · screen unknown"
    );
    assert_eq!(d.ask.len(), 3);
    assert_eq!(d.profile, None);
}

#[test]
fn usb_devices() {
    let d = detect(&Inputs {
        usb: &usb("corne_zmk_studio.json"),
        ..Default::default()
    });
    assert_eq!(d.keyboard.value, Some(Known::Known(KeyboardId::Corne)));
    assert_eq!(d.keyboard.confidence, Some(Confidence::Weak));
    assert_eq!(d.studio.value, Some(true));
    assert!(d.notes.iter().any(|n| n.text.contains("“Corne xnmrph”")));

    let d = detect(&Inputs {
        usb: &usb("nice_nano_bootloader.json"),
        ..Default::default()
    });
    assert_eq!(d.controller.value, Some(Known::Known(ControllerId::NiceNanoV2)));
    assert_eq!(d.controller.confidence, Some(Confidence::Strong));

    let d = detect(&Inputs {
        usb: &usb("unrelated.json"),
        ..Default::default()
    });
    assert!(d.keyboard.value.is_none() && d.controller.value.is_none() && d.notes.is_empty());
}

#[test]
fn quiz_overrides_weak_config_and_detection_serialises() {
    let b = BuildMatrix::parse("include:\n  - board: nice_nano\n    shield: corne_left\n").unwrap();
    let quiz = QuizAnswers {
        controller: Some(ControllerId::NiceNanoV2),
        display: Some(DisplayKind::None),
        ..Default::default()
    };
    let d = detect(&Inputs {
        build: Some(&b),
        quiz: Some(&quiz),
        ..Default::default()
    });
    assert_eq!(d.controller.value, Some(Known::Known(ControllerId::NiceNanoV2)));
    assert_eq!(
        d.controller.confidence,
        Some(Confidence::Medium),
        "weak build.yaml + medium quiz"
    );
    assert_eq!(d.display.value, Some(DisplayKind::None));
    let json = serde_json::to_value(&d).unwrap();
    assert_eq!(
        json["controller"]["value"],
        serde_json::json!({"type": "known", "id": "nice_nano_v2"})
    );
    assert_eq!(json["display"]["value"], "none");
}

#[test]
fn quiz_questions_come_from_the_catalog() {
    let q = mxkeys_hardware::detect::quiz::questions();
    assert_eq!(q[0].options[0].value, "nice_nano_v2");
    assert!(q[0].options[0].supported);
    assert_eq!(
        q[1].options.iter().map(|o| o.value.as_str()).collect::<Vec<_>>(),
        ["nice_view", "oled128x32", "none"]
    );
}
