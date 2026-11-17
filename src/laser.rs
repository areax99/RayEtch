//! GRBL serial connection in a background thread: send-and-wait-for-ok streaming,
//! realtime commands, and status polling.

use crate::i18n::{tr, trf};
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

pub enum Cmd {
    Connect(String, u32),
    Disconnect,
    /// Queue lines to send (job, jog, console)
    Lines(Vec<String>, bool),
    /// Single realtime byte: '?', '!', '~', 0x18
    Realtime(u8),
    StopJob,
}

pub enum Ev {
    Connected(String),
    Disconnected(String),
    Received(String),
    Sent(String),
    Progress(usize, usize),
    JobDone,
    /// `pos`: the machine position (MPos); `work`: the work position (WPos). GRBL reports one of
    /// them; the other comes from the last work offset (WCO) it sent.
    Status { state: String, pos: Option<[f64; 2]>, work: Option<[f64; 2]> },
    /// A port that could not be opened for a reason the person can fix (permission, busy)
    PortProblem(String),
    /// Linux: the system refused the port for lack of permission (the app offers to fix it)
    PortDenied(String),
}

pub struct Laser {
    tx: Sender<Cmd>,
    rx: Receiver<Ev>,
}

impl Laser {
    pub fn new(ctx: eframe::egui::Context) -> Self {
        let (tx, crx) = channel::<Cmd>();
        let (etx, rx) = channel::<Ev>();
        std::thread::spawn(move || worker(crx, etx, ctx));
        Laser { tx, rx }
    }
    pub fn send(&self, c: Cmd) {
        let _ = self.tx.send(c);
    }
    pub fn poll(&self) -> Vec<Ev> {
        self.rx.try_iter().collect()
    }
}

/// Stop: how long the feed hold has to slow the head before the reset (estimated).
pub const STOP_HOLD_MS: u64 = 300;
/// Stop: the longest wait for GRBL to restart after the reset (ESP32 boards take a few seconds).
pub const RESET_WAIT_MS: u64 = 4000;

/// How often the background list of serial ports is refreshed.
pub const PORT_SCAN_EVERY: Duration = Duration::from_secs(2);

/// The serial ports, and which of them are USB, as last read by the background scanner.
#[derive(Default, Clone)]
struct PortList {
    all: Vec<String>,
    usb: Vec<String>,
    /// at least one scan has finished
    ready: bool,
}

/// The serial ports are listed on a thread of their own, every PORT_SCAN_EVERY, and the window
/// only reads the last list. On Windows asking the system for its ports (SetupAPI, Bluetooth
/// included) can take hundreds of milliseconds, and the window used to ask twice every 3 s
/// while looking for the laser: the person's PC froze on Windows and ran smoothly on Linux.
fn port_list() -> &'static std::sync::Mutex<PortList> {
    static LIST: std::sync::OnceLock<std::sync::Mutex<PortList>> = std::sync::OnceLock::new();
    LIST.get_or_init(|| {
        std::thread::spawn(|| loop {
            let ports = serialport::available_ports().unwrap_or_default();
            let all = ports.iter().map(|p| p.port_name.clone()).collect();
            let usb = ports.iter().filter(|p| matches!(p.port_type, serialport::SerialPortType::UsbPort(_))).map(|p| p.port_name.clone()).collect();
            if let Ok(mut l) = port_list().lock() {
                *l = PortList { all, usb, ready: true };
            }
            std::thread::sleep(PORT_SCAN_EVERY);
        });
        std::sync::Mutex::new(PortList::default())
    })
}

/// The serial ports (from the background scan; empty until the first scan is done).
pub fn list_ports() -> Vec<String> {
    port_list().lock().map(|l| l.all.clone()).unwrap_or_default()
}

/// Whether the first background scan of the ports has finished.
pub fn ports_ready() -> bool {
    port_list().lock().map(|l| l.ready).unwrap_or(false)
}

/// The udev rule that opens USB serial ports to the person at the computer (Linux).
pub const LINUX_UDEV_RULE: &str = include_str!("../packaging/linux/70-rayetch-laser.rules");
/// Where the rule goes.
pub const LINUX_UDEV_PATH: &str = "/etc/udev/rules.d/70-rayetch-laser.rules";

/// Linux: give this user access to USB serial lasers without the dialout group or logging out.
/// The system's own password window (pkexec) asks once; then the udev rule is installed, udev
/// re-reads its rules and re-applies them to the serial ports already plugged in, and `port`
/// (the one that was refused) is opened up right away too. Blocks until the person answers.
pub fn linux_grant_access(port: &str) -> Result<(), String> {
    let q = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
    let rule = LINUX_UDEV_RULE.replace('\'', "");
    let script = format!(
        "printf '%s' {rule} > {path} && chmod 644 {path} && \
         (udevadm control --reload-rules || true) && \
         (udevadm trigger --subsystem-match=tty --action=add || true) && \
         (udevadm settle --timeout=5 || true) && \
         ([ ! -e {port} ] || chmod a+rw {port})",
        rule = q(&rule),
        path = q(LINUX_UDEV_PATH),
        port = q(port),
    );
    let out = std::process::Command::new("pkexec").args(["sh", "-c", &script]).output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound { tr("pkexec non è installato: serve per chiedere la password").to_string() } else { e.to_string() }
    })?;
    if out.status.success() {
        Ok(())
    } else {
        // 126: the password window was closed; 127: not authorised
        let code = out.status.code().unwrap_or(-1);
        if code == 126 || code == 127 {
            Err(tr("Password non inserita: l'accesso non è stato concesso").into())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
        }
    }
}

/// The ports the system says are USB devices, whatever their name (a laser board on USB).
pub fn usb_ports() -> Vec<String> {
    port_list().lock().map(|l| l.usb.clone()).unwrap_or_default()
}

fn worker(rx: Receiver<Cmd>, tx: Sender<Ev>, ctx: eframe::egui::Context) {
    let mut port: Option<Box<dyn serialport::SerialPort>> = None;
    let mut queue: VecDeque<String> = VecDeque::new();
    let mut job_total = 0usize;
    let mut job_sent = 0usize;
    let mut in_job = false;
    let mut waiting_ok = false;
    let mut buf = String::new();
    // the work coordinate offset GRBL last reported (WCO), to turn MPos into WPos and back
    let mut wco: [f64; 2] = [0.0, 0.0];
    let mut last_status = Instant::now();
    // GRBL is locked (alarm): the next job unlocks it first
    let mut alarm = false;
    // the last status passed on to the window, so an unchanged one isn't sent again
    let mut last_report: Option<(String, Option<[f64; 2]>, Option<[f64; 2]>)> = None;
    // after a stop's reset, nothing is sent until GRBL greets or this time passes
    let mut resetting_until: Option<Instant> = None;
    let emit = |ev: Ev, tx: &Sender<Ev>| {
        let _ = tx.send(ev);
        ctx.request_repaint();
    };
    loop {
        // commands
        let timeout = if port.is_some() { Duration::from_millis(5) } else { Duration::from_millis(100) };
        match rx.recv_timeout(timeout) {
            Ok(cmd) => match cmd {
                Cmd::Connect(name, baud) => {
                    // a new port: its first status always reaches the window (it confirms the laser)
                    last_report = None;
                    port = None;
                    // DTR and RTS on, as macOS and Linux do when a port opens: on Windows the
                    // serial library leaves both off, and many laser boards (CH340 Arduinos, USB
                    // CDC boards) then never answer, so the laser was not found there
                    match serialport::new(&name, baud).timeout(Duration::from_millis(5)).dtr_on_open(true).open() {
                        Ok(mut p) => {
                            let _ = p.write_request_to_send(true);
                            // a blank line wakes boards that don't restart on opening; the status
                            // queries below then get an answer
                            let _ = p.write_all(b"\r\n");
                            port = Some(p);
                            queue.clear();
                            waiting_ok = false;
                            emit(Ev::Connected(name), &tx);
                        }
                        Err(e) => {
                            // a port the system won't open (no permission, or another program has
                            // it) is said plainly instead of passed over in silence
                            let low = e.to_string().to_lowercase();
                            let hint = if low.contains("permission") || low.contains("access is denied") || low.contains("accesso negato") {
                                if cfg!(windows) {
                                    Some(trf("La porta {} è occupata: chiudi gli altri programmi che usano il laser (LightBurn, LaserGRBL…).", &[&name]))
                                } else {
                                    // Linux and other Unix: the app offers to open the port up
                                    emit(Ev::PortDenied(name.clone()), &tx);
                                    continue;
                                }
                            } else if low.contains("busy") || low.contains("in use") {
                                Some(trf("La porta {} è occupata: chiudi gli altri programmi che usano il laser (LightBurn, LaserGRBL…).", &[&name]))
                            } else {
                                None
                            };
                            match hint {
                                Some(h) => emit(Ev::PortProblem(h), &tx),
                                None => emit(Ev::Disconnected(trf("Impossibile aprire {}: {}", &[&name, &e])), &tx),
                            }
                        }
                    }
                }
                Cmd::Disconnect => {
                    port = None;
                    queue.clear();
                    in_job = false;
                    emit(Ev::Disconnected(tr("Scollegato").into()), &tx);
                }
                Cmd::Lines(lines, is_job) => {
                    if port.is_none() {
                        emit(Ev::Received(tr("Nessun laser collegato.").into()), &tx);
                    } else {
                        if is_job {
                            in_job = true;
                            job_total = lines.len();
                            job_sent = 0;
                            // a job started while GRBL is locked in an alarm unlocks it first ($X),
                            // instead of every line being refused with error:9
                            if alarm {
                                queue.push_back("$X".into());
                                alarm = false;
                            }
                        }
                        queue.extend(lines.into_iter().map(|l| l.trim().to_string()).filter(|l| !l.is_empty() && !l.starts_with(';')));
                        if is_job {
                            job_total = queue.len();
                        }
                    }
                }
                Cmd::Realtime(b) => {
                    if let Some(p) = port.as_mut() {
                        let _ = p.write_all(&[b]);
                    }
                }
                Cmd::StopJob => {
                    queue.clear();
                    in_job = false;
                    waiting_ok = false;
                    if let Some(p) = port.as_mut() {
                        // GRBL's safe stop: a feed hold first, so the head slows to a halt and
                        // keeps its position, then the reset. A reset while moving puts GRBL in
                        // ALARM 3 and it refuses every command until $X or $H (the person had
                        // to press Home and Origine before Avvia worked again)
                        let _ = p.write_all(b"!");
                        std::thread::sleep(Duration::from_millis(STOP_HOLD_MS));
                        let _ = p.write_all(&[crate::gcode::ABORT_BYTE]);
                        // nothing more is sent until GRBL has restarted and greeted
                        resetting_until = Some(Instant::now() + Duration::from_millis(RESET_WAIT_MS));
                        buf.clear();
                    }
                    emit(Ev::JobDone, &tx);
                }
            },
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => return,
        }
        let Some(p) = port.as_mut() else { continue };
        // read
        let mut chunk = [0u8; 512];
        match p.read(&mut chunk) {
            Ok(n) if n > 0 => {
                buf.push_str(&String::from_utf8_lossy(&chunk[..n]));
                while let Some(i) = buf.find('\n') {
                    let line = buf[..i].trim().to_string();
                    buf.drain(..=i);
                    if line.is_empty() {
                        continue;
                    }
                    if line.starts_with('<') {
                        let inner = line.trim_matches(|c| c == '<' || c == '>');
                        let mut parts = inner.split('|');
                        let state = parts.next().unwrap_or("").to_string();
                        let (mut mpos, mut wpos) = (None, None);
                        let xy = |v: &str| -> Option<[f64; 2]> {
                            let n: Vec<f64> = v.split(',').filter_map(|x| x.parse().ok()).collect();
                            (n.len() >= 2).then(|| [n[0], n[1]])
                        };
                        for part in parts {
                            if let Some(v) = part.strip_prefix("MPos:") {
                                mpos = xy(v);
                            } else if let Some(v) = part.strip_prefix("WPos:") {
                                wpos = xy(v);
                            } else if let Some(v) = part.strip_prefix("WCO:") {
                                if let Some(o) = xy(v) {
                                    wco = o;
                                }
                            }
                        }
                        let pos = mpos.or(wpos.map(|w| [w[0] + wco[0], w[1] + wco[1]]));
                        let work = wpos.or(mpos.map(|m| [m[0] - wco[0], m[1] - wco[1]]));
                        // the machine says whether it is locked
                        if state.starts_with("Alarm") {
                            alarm = true;
                        } else if state.starts_with("Idle") || state.starts_with("Run") {
                            alarm = false;
                        }
                        // only a change is passed on: an idle laser answering "?" every 300 ms made
                        // the window redraw several times a second for nothing (slow on old PCs)
                        let now = (state.clone(), pos, work);
                        if last_report.as_ref() != Some(&now) {
                            last_report = Some(now);
                            emit(Ev::Status { state, pos, work }, &tx);
                        }
                        continue;
                    }
                    // GRBL greeted after a reset: it takes commands again
                    if line.starts_with("Grbl") || line.starts_with("GrblHAL") {
                        resetting_until = None;
                        waiting_ok = false;
                    }
                    // locked: an alarm, or the hint GRBL gives at start when it needs unlocking
                    if line.starts_with("ALARM") || line.contains("'$X' to unlock") || line.contains("$X to unlock") {
                        alarm = true;
                    }
                    if line == "ok" || line.starts_with("error") {
                        waiting_ok = false;
                    }
                    if line.starts_with("ALARM") {
                        queue.clear();
                        in_job = false;
                        waiting_ok = false;
                    }
                    if line != "ok" || !in_job {
                        emit(Ev::Received(line), &tx);
                    }
                }
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(e) => {
                port = None;
                emit(Ev::Disconnected(trf("Collegamento perso: {}", &[&e])), &tx);
                continue;
            }
        }
        // write
        // after a reset, wait for GRBL to greet (or the time to run out) before sending more
        if resetting_until.is_some_and(|t| Instant::now() < t) {
            continue;
        }
        resetting_until = None;
        if !waiting_ok {
            if let Some(line) = queue.pop_front() {
                if p.write_all(format!("{line}\n").as_bytes()).is_ok() {
                    waiting_ok = true;
                    if in_job {
                        job_sent += 1;
                        if job_sent % 20 == 0 || job_sent == job_total {
                            emit(Ev::Progress(job_sent, job_total), &tx);
                        }
                    } else {
                        emit(Ev::Sent(line), &tx);
                    }
                }
            } else if in_job {
                in_job = false;
                emit(Ev::JobDone, &tx);
            }
        }
        if last_status.elapsed() > Duration::from_millis(300) {
            last_status = Instant::now();
            let _ = p.write_all(b"?");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait_for(l: &Laser, secs: u64, mut f: impl FnMut(&Ev) -> bool) -> bool {
        let end = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < end {
            for e in l.poll() {
                match &e {
                    Ev::Received(t) => println!("HW < {t}"),
                    Ev::Sent(t) => println!("HW > {t}"),
                    Ev::Status { state, pos, .. } => println!("HW status {state} {pos:?}"),
                    Ev::Connected(p) => println!("HW connected {p}"),
                    Ev::Disconnected(r) => println!("HW disconnected {r}"),
                    Ev::Progress(a, b) => println!("HW progress {a}/{b}"),
                    Ev::JobDone => println!("HW job done"),
                    Ev::PortProblem(m) => println!("HW port problem {m}"),
                    Ev::PortDenied(p) => println!("HW port denied {p}"),
                }
                if f(&e) {
                    return true;
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    #[test]
    fn auto_search_tries_only_usb_ports() {
        let ports: Vec<String> = ["/dev/cu.Bluetooth-Incoming-Port", "/dev/cu.debug-console", "/dev/cu.JBLCharge3",
            "/dev/cu.JBLTUNE510BT", "/dev/cu.usbserial-10", "/dev/tty.usbserial-10"].iter().map(|s| s.to_string()).collect();
        if !cfg!(windows) {
            assert_eq!(crate::app::auto_port_order("", &ports), vec!["/dev/cu.usbserial-10".to_string()]);
        }
        assert_eq!(crate::app::auto_port_order("/dev/cu.JBLCharge3", &ports), vec!["/dev/cu.JBLCharge3".to_string()]);
    }

    /// Real-hardware check of the streaming worker, run on request with BF_PORT set.
    /// Connects, reads the status and build info, then disconnects; nothing moves.
    #[test]
    #[ignore]
    fn hw_worker_connects_and_queries() {
        let port = std::env::var("BF_PORT").expect("BF_PORT");
        let l = Laser::new(eframe::egui::Context::default());
        l.send(Cmd::Connect(port, 115200));
        assert!(wait_for(&l, 3, |e| matches!(e, Ev::Connected(_))));
        assert!(wait_for(&l, 4, |e| matches!(e, Ev::Status { .. })), "no status report");
        l.send(Cmd::Lines(vec!["$I".into(), "$G".into()], false));
        assert!(wait_for(&l, 4, |e| matches!(e, Ev::Received(t) if t.starts_with("[GC:"))), "no $G answer");
        l.send(Cmd::Disconnect);
        assert!(wait_for(&l, 2, |e| matches!(e, Ev::Disconnected(_))));
    }

    /// Real-hardware motion check (beam off, every move S0), run on request with BF_PORT set:
    /// unlock, jog, box frame and shapes-only frame of two squares, then back to where it started.
    #[test]
    #[ignore]
    fn hw_motion_frames_with_beam_off() {
        use crate::gcode::*;
        use crate::model::*;
        let port = std::env::var("BF_PORT").expect("BF_PORT");
        let l = Laser::new(eframe::egui::Context::default());
        l.send(Cmd::Connect(port, 115200));
        assert!(wait_for(&l, 3, |e| matches!(e, Ev::Connected(_))));
        let mut start = [0.0, 0.0];
        assert!(wait_for(&l, 4, |e| if let Ev::Status { pos: Some(p), .. } = e { start = *p; true } else { false }));
        let idle = |l: &Laser| wait_for(l, 30, |e| matches!(e, Ev::Status { state, .. } if state == "Idle"));
        let run = |l: &Laser, lines: Vec<String>| {
            assert!(lines.iter().all(|x| !x.contains("M3") && !x.contains("M4") && (!x.starts_with("G1") || x.contains("S0"))));
            l.send(Cmd::Lines(lines, true));
            assert!(wait_for(l, 30, |e| matches!(e, Ev::JobDone)), "job not acknowledged");
            std::thread::sleep(Duration::from_millis(400));
            assert!(idle(l), "machine did not return to Idle");
        };
        l.send(Cmd::Lines(vec!["$X".into()], false));
        std::thread::sleep(Duration::from_millis(300));
        run(&l, vec![jog_line(false, 10.0, 10.0, 3000.0)]);
        run(&l, vec![jog_line(false, -10.0, -10.0, 3000.0)]);
        let mut doc = Document::default();
        doc.add_shape(Shape::new(0, Geometry::Rect { w: 10.0, h: 10.0, radius: 0.0 }, 0, 30.0, 30.0));
        doc.add_shape(Shape::new(0, Geometry::Rect { w: 10.0, h: 10.0, radius: 0.0 }, 0, 70.0, 60.0));
        let opts = JobOptions { start_from: StartFrom::AbsoluteCoords, job_origin: 6, selected_only: false, optimize: true, selection_origin: false };
        let job = plan(&doc, &[], &opts);
        let f = frame_gcode(&doc, &job, &opts, 3000.0, "0");
        println!("HW frame {f:?}");
        run(&l, f);
        let o = outline_frame_gcode(&doc, &job, &opts, 0.0, "0");
        println!("HW outline {o:?}");
        run(&l, o);
        run(&l, vec![jog_line(true, start[0], start[1], 3000.0)]);
        l.send(Cmd::Disconnect);
        assert!(wait_for(&l, 2, |e| matches!(e, Ev::Disconnected(_))));
    }
}
