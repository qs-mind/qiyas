//! The Options, Customize and Aliases dialogs' settings: display precision, snap aperture, pick
//! box, colours, keyboard shortcuts, the ribbon layout and aliases, kept in `settings.json`.

use std::fs;

use qiyas_desktop_lib::ribbon::{self, ButtonKind, RibbonLayout};
use qiyas_desktop_lib::settings::{self, Settings};

fn custom_ribbon() -> RibbonLayout {
    let mut layout = ribbon::default_ribbon();
    // Measure is first already; on Draw (id "home") the Edit group keeps its first button only,
    // renamed.
    let home = &mut layout.tabs[1];
    assert_eq!(home.id, "home");
    home.groups[2].buttons.truncate(1);
    home.groups[2].label = "My drawing".into();
    layout
}

#[test]
fn options_have_defaults_and_an_old_file_takes_them() {
    let s = Settings::default();
    assert_eq!(s.display_precision, 3);
    assert_eq!((s.snap_aperture_px, s.pick_box_px), (10, 6));
    assert_eq!(
        (&s.background_color, &s.crosshair_color, &s.highlight_color),
        (&None, &None, &None)
    );
    assert!(s.shortcuts.is_empty());
    assert_eq!(s.ribbon, None);
    assert!(settings::validate(&s).is_ok());

    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("settings.json"),
        r#"{"theme":"dark","aliases":{"LO":"LAYOFF"}}"#,
    )
    .unwrap();
    let old = settings::load(dir.path());
    assert_eq!(old.theme, "dark");
    assert_eq!(old.display_precision, 3);
    assert_eq!(old.ribbon, None);
}

#[test]
fn options_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let s = Settings {
        display_precision: 2,
        snap_aperture_px: 15,
        pick_box_px: 4,
        background_color: Some("#0e0f12".into()),
        crosshair_color: Some("#FFFFFF".into()),
        highlight_color: Some("#c9a6e0".into()),
        shortcuts: [
            ("line".to_string(), "L".to_string()),
            ("copybase".to_string(), "Ctrl+Shift+C".to_string()),
            ("osnap".to_string(), "F3".to_string()),
        ]
        .into(),
        ribbon: Some(custom_ribbon()),
        aliases: [("LO".to_string(), "LAYOFF".to_string())].into(),
        ..Settings::default()
    };
    assert_eq!(settings::validate(&s), Ok(()));
    settings::save(dir.path(), &s).unwrap();
    let text = fs::read_to_string(dir.path().join("settings.json")).unwrap();
    assert!(text.contains("\"snap_aperture_px\": 15"), "{text}");
    assert!(text.contains("\"kind\": \"tool\""));
    assert_eq!(settings::load(dir.path()), s);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(dir.path().join("settings.json"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    // Null is the default layout.
    let reset = Settings { ribbon: None, ..s };
    settings::save(dir.path(), &reset).unwrap();
    assert!(fs::read_to_string(dir.path().join("settings.json"))
        .unwrap()
        .contains("\"ribbon\": null"));
    assert_eq!(settings::load(dir.path()).ribbon, None);
}

#[test]
fn options_out_of_range_are_refused() {
    let good = Settings::default();
    let refused = |s: Settings| settings::validate(&s).unwrap_err();
    assert!(refused(Settings {
        display_precision: 5,
        ..good.clone()
    })
    .contains("precision"));
    assert!(refused(Settings {
        snap_aperture_px: 0,
        ..good.clone()
    })
    .contains("snap aperture"));
    assert!(refused(Settings {
        pick_box_px: 51,
        ..good.clone()
    })
    .contains("pick box"));
    for bad in ["red", "#12345", "#12345g", "0e0f12"] {
        assert!(refused(Settings {
            highlight_color: Some(bad.into()),
            ..good.clone()
        })
        .contains(bad));
    }
    assert!(refused(Settings {
        aliases: [
            ("lo".to_string(), "LAYOFF".to_string()),
            ("LO".to_string(), "LAYON".to_string())
        ]
        .into(),
        ..good.clone()
    })
    .contains("twice"));
}

#[test]
fn shortcuts_are_keys_and_never_share_one() {
    for (key, chord) in [
        ("d", "D"),
        ("shift+ctrl+c", "Ctrl+Shift+C"),
        ("F12", "F12"),
        ("Ctrl+9", "Ctrl+9"),
        ("alt+arrowup", "Alt+ArrowUp"),
    ] {
        assert_eq!(settings::shortcut_key(key).as_deref(), Some(chord), "{key}");
    }
    for bad in [
        "",
        "Ctrl+",
        "Ctrl+Ctrl+C",
        "Hyper+C",
        "F25",
        "F0",
        "F01",
        "CC",
        "Ctrl C",
    ] {
        assert_eq!(settings::shortcut_key(bad), None, "{bad}");
    }
    let good = Settings::default();
    let clash = Settings {
        shortcuts: [
            ("copybase".to_string(), "Ctrl+Shift+C".to_string()),
            ("copyclip".to_string(), "shift+ctrl+c".to_string()),
        ]
        .into(),
        ..good.clone()
    };
    assert!(settings::validate(&clash).unwrap_err().contains("both"));
    let bad = Settings {
        shortcuts: [("line".to_string(), "Ctrl+".to_string())].into(),
        ..good
    };
    assert!(settings::validate(&bad).is_err());
}

#[test]
fn the_default_ribbon_is_valid_and_holds_every_tab() {
    let layout = ribbon::default_ribbon();
    assert_eq!(ribbon::validate(&layout), Ok(()));
    let tabs: Vec<&str> = layout.tabs.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(
        tabs,
        [
            "measure",
            "home",
            "annotate",
            "parametric",
            "insert",
            "view",
            "manage",
            "output"
        ]
    );
    let buttons: Vec<_> = layout
        .tabs
        .iter()
        .flat_map(|t| &t.groups)
        .flat_map(|g| &g.buttons)
        .collect();
    for (kind, command) in [
        (ButtonKind::Tool, "line"),
        (ButtonKind::Tool, "trim"),
        (ButtonKind::Tool, "dimLinear"),
        (ButtonKind::Action, "audit"),
        (ButtonKind::Action, "purge"),
        (ButtonKind::Action, "runScript"),
        (ButtonKind::Action, "plot"),
        (ButtonKind::Action, "quickPrint"),
        (ButtonKind::Action, "printAll"),
        (ButtonKind::Action, "plotStyles"),
        (ButtonKind::Toggle, "arabicReversed"),
    ] {
        assert!(
            buttons
                .iter()
                .any(|b| b.kind == kind && b.command == command),
            "{command}"
        );
    }
    // The JSON the window reads: Draw (id "home") starts with Draw, Line; Measure comes first and
    // its Dimensions group shows five buttons.
    let json = serde_json::to_value(&layout).unwrap();
    assert_eq!(
        json["tabs"][1]["groups"][0]["buttons"][0]["command"],
        "line"
    );
    assert_eq!(json["tabs"][1]["groups"][0]["buttons"][0]["kind"], "tool");
    assert_eq!(json["tabs"][0]["groups"][3]["main"], 5);
    assert!(json["tabs"][1]["groups"][0].get("main").is_none());
    let home: Vec<&str> = layout.tabs[1]
        .groups
        .iter()
        .map(|g| g.id.as_str())
        .collect();
    assert_eq!(
        home,
        ["draw", "modify", "edit", "text", "clipboard", "layers"]
    );
    let measure: Vec<&str> = layout.tabs[0]
        .groups
        .iter()
        .map(|g| g.id.as_str())
        .collect();
    assert_eq!(measure, ["measure", "inquiry", "takeoff", "dims", "markup"]);
    // Annotate: the Markup group (id "markup") is where a revision cloud with a comment goes.
    let annotate: Vec<&str> = layout.tabs[2]
        .groups
        .iter()
        .map(|g| g.id.as_str())
        .collect();
    assert_eq!(annotate, ["dims", "notes", "markup", "tables"]);
    // New, Open, Save, Save as, Undo and Redo are the window's quick-access bar, in no layout.
    for command in ["new", "open", "save", "saveAs", "undo", "redo"] {
        assert!(buttons.iter().all(|b| b.command != command), "{command}");
    }
}

#[test]
fn a_saved_ribbon_may_move_buttons_between_groups_and_tabs_but_not_add_the_quick_access_ones() {
    let mut layout = ribbon::default_ribbon();
    // Compare from Measure's Take-off to Home's Draw, Recover from Manage to a new Home group,
    // and Draw's Line to the end of Modify.
    let takeoff = &mut layout.tabs[0].groups[2];
    let at = takeoff
        .buttons
        .iter()
        .position(|b| b.command == "compare")
        .unwrap();
    let compare = takeoff.buttons.remove(at);
    let manage = layout.tabs.iter_mut().find(|t| t.id == "manage").unwrap();
    let drawing = manage
        .groups
        .iter_mut()
        .find(|g| g.id == "drawing")
        .unwrap();
    let recover = drawing.buttons.pop().unwrap();
    assert_eq!(recover.command, "recover");
    let home = &mut layout.tabs[1];
    let line = home.groups[0].buttons.remove(0);
    home.groups[0].buttons.push(compare);
    home.groups[1].buttons.push(line);
    home.groups.push(ribbon::RibbonGroup {
        id: "mine".into(),
        label: "My files".into(),
        buttons: vec![recover],
        main: None,
    });
    assert_eq!(ribbon::validate(&layout), Ok(()));
    let dir = tempfile::tempdir().unwrap();
    let s = Settings {
        ribbon: Some(layout.clone()),
        ..Settings::default()
    };
    settings::save(dir.path(), &s).unwrap();
    assert_eq!(settings::load(dir.path()).ribbon, Some(layout.clone()));

    // A layout from before the quick-access bar, with Open in Home, is not one this version
    // draws: it gives way to the default on load.
    let mut old = layout;
    old.tabs[0].groups[0].buttons.push(ribbon::RibbonButton {
        kind: ButtonKind::Action,
        command: "open".into(),
        label: "open".into(),
    });
    assert!(ribbon::validate(&old).unwrap_err().contains("open"));
}

#[test]
fn the_default_ribbon_is_the_one_the_window_draws() {
    // `src/shell/ribbon.default.json`: `DEFAULT_RIBBON` in `src/shell/ribbonLayout.ts` is pinned
    // to it by `ribbonLayout.test.ts`, so the two defaults can't drift apart.
    let window: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../src/shell/ribbon.default.json"
    )))
    .unwrap();
    assert_eq!(
        serde_json::to_value(ribbon::default_ribbon()).unwrap(),
        window
    );
}

#[test]
fn a_ribbon_with_an_unknown_button_or_twin_ids_is_refused() {
    let mut unknown = ribbon::default_ribbon();
    unknown.tabs[0].groups[0].buttons[0].command = "format_disk".into();
    assert!(ribbon::validate(&unknown)
        .unwrap_err()
        .contains("format_disk"));
    // A tool's command run as an action is not a button the window knows.
    let mut wrong_kind = ribbon::default_ribbon();
    wrong_kind.tabs[1].groups[3].buttons[0].kind = ButtonKind::Action;
    assert!(ribbon::validate(&wrong_kind).is_err());
    let mut twins = ribbon::default_ribbon();
    twins.tabs[0].id = "home".into();
    assert!(ribbon::validate(&twins).unwrap_err().contains("home"));
    let empty = RibbonLayout { tabs: Vec::new() };
    assert!(ribbon::validate(&empty).is_err());
    assert!(settings::validate(&Settings {
        ribbon: Some(unknown),
        ..Settings::default()
    })
    .is_err());
}

#[test]
fn a_saved_ribbon_this_version_cannot_draw_gives_way_to_the_default() {
    let dir = tempfile::tempdir().unwrap();
    let mut layout = ribbon::default_ribbon();
    layout.tabs[0].groups[0].buttons[0].command = "gone_since".into();
    let s = Settings {
        ribbon: Some(layout),
        ..Settings::default()
    };
    settings::save(dir.path(), &s).unwrap();
    let loaded = settings::load(dir.path());
    assert_eq!(loaded.ribbon, None);
    assert_eq!(loaded.mcp_token, s.mcp_token, "the rest is kept");
    // And written back, so the next start reads it clean.
    assert!(!fs::read_to_string(dir.path().join("settings.json"))
        .unwrap()
        .contains("gone_since"));
}
