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
