//! The link to the buds on its own thread: finds them, opens the socket and runs the OPPO driver
//! (`buds_core::oppo::driver`) on it. The driver holds the protocol; this file holds the I/O, the clock and the
//! settings file. Nothing is polled after connect.

use crate::bt;
use buds_core::oppo::driver::{Driver, Effect, Settings};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub use buds_core::oppo::driver::{Cmd, Snapshot, Status};
use buds_core::oppo::types::{is_known_name, HearingEv};

/// A monotonic clock for the driver: the desktop can go longer than 49 days.
fn now_ms() -> u64 {
    static T0: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    T0.get_or_init(Instant::now).elapsed().as_millis() as u64
}

fn unix_s() -> u32 { SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as u32) }

/// The driver's settings from `settings.json`, for the buds at `addr` (0: none yet).
fn settings(addr: u64) -> Settings {
    let all = crate::load_settings();
    Settings {
        product_id: all["product_id"].as_str().map(Into::into),
        model_manual: all["model_manual"].as_str().map(Into::into),
        clear_battery: all["clear_battery"].as_bool() == Some(true),
        // Only OPPO and OnePlus phones make 3D audio themselves.
        phone_spatial: false,
        case_level: all["case_levels"][format!("{addr:012X}")].as_u64().map(|l| l as u8),
        // The desktop pages show the EQ at once: the init sequence reads it.
        eq_on_connect: true,
        eq_saved: None,
    }
}

/// The last real case level of the buds at `addr`, kept per address (`settings.json` "case_levels").
fn save_case_level(addr: u64, level: serde_json::Value) {
    let mut all = crate::load_settings()["case_levels"].as_object().cloned().unwrap_or_default();
    all.insert(format!("{addr:012X}"), level);
    crate::save_setting("case_levels", all.into());
}

type Emit = Box<dyn Fn(Snapshot) + Send>;

/// One packet log line (Dev tools).
pub struct Line {
    /// Local time, "18:23:05.123".
    pub at: String,
    /// "TX", "RX", "DISCARDED RX", or "NOTE" (a connection change in words, no bytes).
    pub dir: &'static str,
    pub bytes: Vec<u8>,
    /// The decoded description; None = a packet nothing here names (amber in Human).
    pub human: Option<String>,
}

// ponytail: unbounded; a cap if a days-long session ever shows in memory.
pub static LOG: Mutex<Vec<Line>> = Mutex::new(Vec::new());

/// Hearing profile replies and pushes, in order, for the page and the test to take (`hearing::apply`).
pub static HEARING: Mutex<Vec<HearingEv>> = Mutex::new(Vec::new());

pub(crate) fn log(dir: &'static str, bytes: Vec<u8>, human: Option<String>) {
    let [.., h, mi, sec, ms] = crate::local_now();
    let line = Line { at: format!("{h:02}:{mi:02}:{sec:02}.{ms:03}"), dir, bytes, human };
    crate::crash::line(&crate::report::line_text(&line));
    LOG.lock().unwrap().push(line);
}

pub fn spawn(emit: impl Fn(Snapshot) + Send + 'static) -> Sender<Cmd> {
    let (tx, rx) = channel();
    std::thread::spawn(move || run(rx, Box::new(emit)));
    tx
}

enum End { Lost, UserDisconnect, Quit }

/// Addresses that answered as buds before (`settings.json` "buds"), for systems that do not list the vendor service.
fn remembered() -> Vec<u64> {
    crate::load_settings()["buds"].as_array().into_iter().flatten()
        .filter_map(|v| u64::from_str_radix(v.as_str()?, 16).ok()).collect()
}

fn remember(addr: u64) {
    let mut list = remembered();
    if addr == 0 || list.contains(&addr) { return; }
    list.push(addr);
    crate::save_setting("buds", list.iter().map(|a| format!("{a:012X}")).collect::<Vec<_>>().into());
}

fn run(rx: Receiver<Cmd>, emit: Emit) {
    let mut paused = false;
    let mut manual = false;
    let mut driver = Driver::new(settings(0));
    let mut last_err = String::new();
    loop {
        if !paused {
            // Auto-connect follows audio: only buds the system has a link to. Buds are found by the vendor's
            // control service (`BudsDevice.find`), or as the device that answered before; a known model name
            // is the last resort. A user Connect also tries any other connected device.
            let known = remembered();
            let rank = |d: &bt::Device| if d.vendor || known.contains(&d.addr) { 0 } else if is_known_name(&d.name) { 1 } else { 2 };
            // A user Connect first asks the system to bring up the audio of known buds that have no link.
            if manual {
                for d in bt::paired().iter().filter(|d| !d.connected && rank(d) < 2) { bt::bring_up(d.addr); }
            }
            let mut devices: Vec<_> = bt::paired().into_iter().filter(|d| d.connected && (manual || rank(d) < 2)).collect();
            devices.sort_by_key(rank);
            for d in devices {
                emit(Snapshot { status: Status::Connecting, name: d.name.clone(), ..Default::default() });
                let link = match bt::connect(d.addr) {
                    Ok(l) => l,
                    Err(e) => {
                        // One line per new error, not one per 5 s rescan.
                        let msg = format!("Connect to {} failed: {e}", d.name);
                        if msg != last_err { log("NOTE", Vec::new(), Some(msg.clone())); last_err = msg; }
                        continue;
                    }
                };
                last_err.clear();
                log("NOTE", Vec::new(), Some(format!("Connected to {}", d.name)));
                // The link closes at the end of this block.
                let end = Conn { link, addr: d.addr, driver: &mut driver, emit: &emit }.serve(&rx, d.name);
                let outs = driver.disconnected(now_ms());
                Conn::effects(&emit, &driver, d.addr, outs);
                match end {
                    End::Lost => log("NOTE", Vec::new(), Some("Connection lost".into())),
                    End::UserDisconnect => { log("NOTE", Vec::new(), Some("Disconnected".into())); paused = true }
                    End::Quit => return,
                }
                break;
            }
            emit(driver.snapshot().clone());
        }
        manual = false;
        // An active engine needs its failure detector with no link too (a restart wait).
        let outs = driver.tick(now_ms());
        Conn::effects(&emit, &driver, 0, outs);
        // ponytail: a 5 s rescan while idle, 500 ms while the engine waits; a Windows device-change
        // notification if the rescan ever costs.
        let wait = if driver.next_wake().is_some() { Duration::from_millis(500) } else { Duration::from_secs(5) };
        match rx.recv_timeout(wait) {
            Ok(Cmd::Connect) => { paused = false; manual = true; }
            Ok(Cmd::Disconnect) => paused = true,
            Ok(_) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

struct Conn<'a> {
    link: bt::Link,
    /// 0 for the bridge.
    addr: u64,
    driver: &'a mut Driver,
    emit: &'a Emit,
}

impl Conn<'_> {
    /// Runs the driver's effects that need no link, and shows the snapshot when it changed.
    fn effects(emit: &Emit, driver: &Driver, addr: u64, outs: Vec<Effect>) {
        let mut changed = false;
        for e in outs {
            match e {
                Effect::Send(_) => {}
                Effect::Log { dir, bytes, human } => log(dir, bytes, human),
                Effect::State => changed = true,
                Effect::Hearing(h) => HEARING.lock().unwrap().push(h),
                Effect::Save("case_level", value) => save_case_level(addr, value),
                Effect::Save(key, value) => crate::save_setting(key, value),
                // Buds that named their model are remembered, renamed or not.
                Effect::Ready => if driver.snapshot().model.is_some() { remember(addr) },
                // The desktop has no ringer: the driver's NOTE line is the record.
                Effect::FindPhone(_) => {}
            }
        }
        if changed { emit(driver.snapshot().clone()); }
    }

    /// Writes the frames, then runs the other effects. Err when the link is gone.
    fn run_effects(&mut self, outs: Vec<Effect>) -> Result<(), String> {
        let mut rest = Vec::new();
        for e in outs {
            match e {
                Effect::Send(p) => self.link.write(&p)?,
                e => rest.push(e),
            }
        }
        Conn::effects(self.emit, self.driver, self.addr, rest);
        Ok(())
    }

    fn serve(&mut self, rx: &Receiver<Cmd>, name: String) -> End {
        self.driver.set_settings(settings(self.addr));
        let outs = self.driver.connected(now_ms(), name);
        match self.pump(rx, outs) {
            Ok(end) => end,
            Err(_) => End::Lost,
        }
    }

    /// The link loop: the UI's commands, the clock, and the bytes the buds send. The read waits 50 ms at most.
    fn pump(&mut self, rx: &Receiver<Cmd>, first: Vec<Effect>) -> Result<End, String> {
        self.run_effects(first)?;
        let mut buf = [0u8; 1024];
        loop {
            loop {
                match rx.try_recv() {
                    Ok(Cmd::Disconnect) => return Ok(End::UserDisconnect),
                    Ok(Cmd::Connect) => {}
                    Ok(c) => {
                        // A command can follow a settings change (the model pick).
                        self.driver.set_settings(settings(self.addr));
                        let outs = self.driver.command(now_ms(), c);
                        self.run_effects(outs)?;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return Ok(End::Quit),
                }
            }
            let outs = self.driver.tick(now_ms());
            self.run_effects(outs)?;
            let n = self.link.read(&mut buf)?;
            if n > 0 {
                let outs = self.driver.on_bytes(now_ms(), unix_s(), &buf[..n]);
                self.run_effects(outs)?;
            }
        }
    }
}
