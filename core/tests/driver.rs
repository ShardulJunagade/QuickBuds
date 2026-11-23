//! The OPPO driver's step queue: the init timing and the order of UI commands.
use buds_core::oppo::driver::{Cmd, Driver, Effect, Settings};
use buds_core::types::{build_packet, cmd_of};

fn sent(e: &[Effect]) -> Vec<u16> {
    e.iter().filter_map(|e| match e { Effect::Send(p) => Some(cmd_of(p)), _ => None }).collect()
}

#[test]
fn init_waits_300_ms_then_sends_200_ms_apart() {
    let mut d = Driver::new(Settings::default());
    assert!(sent(&d.connected(0, "X".into())).is_empty());
    assert_eq!(d.next_wake(), Some(300));
    assert!(sent(&d.tick(299)).is_empty());
    assert_eq!(sent(&d.tick(300)), [0x0100]);
    assert_eq!(d.next_wake(), Some(500));
    assert_eq!(sent(&d.tick(500)), [0x0103]);
    assert_eq!(sent(&d.tick(700)), [0x0200]);
}

#[test]
fn a_command_waits_for_the_end_of_the_init() {
    let mut d = Driver::new(Settings::default());
    d.connected(0, "X".into());
    assert!(sent(&d.command(10, Cmd::Find(true))).is_empty());
    // The init ends once every read had its wait; then the command goes out.
    let mut all = Vec::new();
    let mut now = 0;
    while let Some(t) = d.next_wake() {
        now = t.max(now + 1);
        all.extend(sent(&d.tick(now)));
        if now > 60_000 { break; }
    }
    assert_eq!(all.last(), Some(&0x0400));
}

#[test]
fn with_no_link_a_command_does_nothing() {
    let mut d = Driver::new(Settings::default());
    assert!(sent(&d.command(0, Cmd::Find(true))).is_empty());
    assert_eq!(d.next_wake(), None);
}

#[test]
fn bytes_before_the_header_are_logged_as_discarded() {
    let mut d = Driver::new(Settings::default());
    d.connected(0, "X".into());
    let mut data = vec![0x01, 0x02];
    data.extend(build_packet(0x8106, 1, &[0, 1, 1, 0x46]));
    let e = d.on_bytes(5, 0, &data);
    assert!(e.iter().any(|e| matches!(e, Effect::Log { dir: "DISCARDED RX", .. })));
}

#[test]
fn common_state_carries_battery_and_connection() {
    use buds_core::model::{Level, Wear};
    let mut d = Driver::new(Settings::default());
    assert!(!d.common().connected);
    d.connected(0, "X".into());
    d.on_bytes(5, 0, &build_packet(0x8106, 1, &[0, 3, 1, 0x46, 2, 0xC6, 3, 0]));
    let c = d.common();
    assert!(c.connected);
    assert_eq!(c.battery[0], Some(Level { value: 0x46, max: 100, charging: false }));
    assert_eq!(c.battery[1], Some(Level { value: 0x46, max: 100, charging: true }));
    assert_eq!(c.battery[2], None);
    assert_eq!(c.wear, [Wear::Unknown; 2]);
    // Nothing read yet: only the rows that need no reply.
    assert!(c.features.contains("earbuds"));
}

#[test]
fn a_case_level_0_shows_the_last_real_one_and_a_real_one_is_saved() {
    use buds_core::model::Level;
    let mut d = Driver::new(Settings { case_level: Some(80), ..Default::default() });
    d.connected(0, "X".into());
    d.on_bytes(5, 0, &build_packet(0x8106, 1, &[0, 1, 3, 0]));
    assert_eq!(d.common().battery[2], Some(Level { value: 80, max: 100, charging: false }));
    let e = d.on_bytes(6, 0, &build_packet(0x8106, 1, &[0, 1, 3, 0x41]));
    assert!(e.iter().any(|e| matches!(e, Effect::Save("case_level", v) if v == 0x41)));
    assert_eq!(d.common().battery[2].map(|l| l.value), Some(0x41));
}

#[test]
fn the_saved_eq_shows_until_the_buds_answer() {
    use buds_core::oppo::driver::EqSaved;
    let mut d = Driver::new(Settings { eq_saved: Some(EqSaved { current: Some(4), bass_level: Some(2), custom: vec![] }), ..Default::default() });
    d.connected(0, "X".into());
    assert_eq!(d.snapshot().eq_current, Some(4));
    assert_eq!(d.snapshot().bass_level, Some(2));
}

#[test]
fn a_stand_in_case_level_shows_while_the_link_gives_none() {
    let mut d = Driver::new(Settings::default());
    d.connected(0, "X".into());
    d.on_bytes(5, 0, &build_packet(0x8106, 1, &[0, 1, 1, 0x5A]));
    d.case_stand_in(10, 80, false);
    assert_eq!(d.common().battery[2].map(|l| l.value), Some(80));
    // The link's own case level wins.
    d.on_bytes(20, 0, &build_packet(0x8106, 1, &[0, 1, 3, 0x41]));
    assert_eq!(d.common().battery[2].map(|l| l.value), Some(0x41));
}

#[test]
fn the_levels_of_the_last_link_show_until_the_buds_report() {
    let mut d = Driver::new(Settings::default());
    d.connected(0, "X".into());
    d.on_bytes(5, 0, &build_packet(0x8106, 1, &[0, 2, 1, 0x5A, 3, 0x3C]));
    d.disconnected(10);
    d.connected(20, "X".into());
    assert_eq!(d.common().battery[2].map(|l| l.value), Some(0x3C));
    // Other buds start empty.
    d.disconnected(30);
    d.connected(40, "Y".into());
    assert_eq!(d.common().battery[2], None);
}
