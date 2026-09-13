use fortinet_vpn_status::{
    classify, config::Config, monitor::Monitor, presentation::Report, Interface, Status,
};
use std::io::{self, BufRead, Read, Write};
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Drain stdout without blocking or leaving reader threads behind in watch mode.
/// The deadline covers both process exit and inherited stdout handles closing.
fn capture(command: &mut Command, timeout: Duration) -> io::Result<String> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let deadline = Instant::now() + timeout;
    let mut stdout = child.stdout.take().expect("stdout is piped");
    let result = (|| {
        // SAFETY: stdout owns a valid file descriptor throughout these calls;
        // fcntl only reads/updates its flags and does not take ownership.
        let flags = unsafe { libc::fcntl(stdout.as_raw_fd(), libc::F_GETFL) };
        if flags == -1
            || unsafe { libc::fcntl(stdout.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) }
                == -1
        {
            return Err(io::Error::last_os_error());
        }
        let mut output = Vec::new();
        let mut eof = false;
        let mut buffer = [0; 8192];
        loop {
            // Bound work per pass so a continuously writing command cannot
            // prevent timeout checks. Neither command should exceed 1 MiB.
            for _ in 0..8 {
                match stdout.read(&mut buffer) {
                    Ok(0) => {
                        eof = true;
                        break;
                    }
                    Ok(n) => {
                        output.extend_from_slice(&buffer[..n]);
                        if output.len() > 1024 * 1024 {
                            return Err(io::ErrorKind::InvalidData.into());
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(error) => return Err(error),
                }
            }
            if let Some(status) = child.try_wait()? {
                if !status.success() {
                    return Err(io::Error::other("status command failed"));
                }
                if eof {
                    return String::from_utf8(output)
                        .map_err(|_| io::ErrorKind::InvalidData.into());
                }
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(io::ErrorKind::TimedOut.into());
            }
            thread::sleep(remaining.min(Duration::from_millis(10)));
        }
    })();
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}

fn command_error(error: io::Error, command: &str) -> &'static str {
    match (command, error.kind()) {
        ("forticlient", io::ErrorKind::NotFound) => "forticlient_missing",
        ("forticlient", io::ErrorKind::TimedOut) => "forticlient_timeout",
        ("forticlient", _) => "forticlient_failed",
        (_, io::ErrorKind::NotFound) => "ip_missing",
        (_, io::ErrorKind::TimedOut) => "ip_timeout",
        _ => "ip_failed",
    }
}

fn probe() -> Result<Status, &'static str> {
    let cli = capture(
        Command::new("forticlient")
            .args(["vpn", "status"])
            .env("LC_ALL", "C"),
        Duration::from_secs(3),
    )
    .map_err(|error| command_error(error, "forticlient"))?;
    let links = capture(
        Command::new("ip").args(["-j", "address"]),
        Duration::from_secs(2),
    )
    .map_err(|error| command_error(error, "ip"))?;
    let interfaces: Vec<Interface> = serde_json::from_str(&links).map_err(|_| "ip_invalid_json")?;
    Ok(classify(&cli, &interfaces))
}

#[derive(Default)]
struct Options {
    watch: bool,
    config: Option<PathBuf>,
}

impl Options {
    fn parse() -> Result<Self, &'static str> {
        let mut options = Self::default();
        let mut args = std::env::args_os().skip(1);
        while let Some(arg) = args.next() {
            if arg == "--watch" && !options.watch {
                options.watch = true;
            } else if arg == "--config" && options.config.is_none() {
                let path = args.next().ok_or("invalid_arguments")?;
                if path.is_empty() || path.to_string_lossy().starts_with("--") {
                    return Err("invalid_arguments");
                }
                options.config = Some(path.into());
            } else {
                return Err("invalid_arguments");
            }
        }
        Ok(options)
    }
}

fn refresh_requests() -> Receiver<()> {
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        for line in io::stdin().lock().lines() {
            match line {
                Ok(line) if line.trim() == "refresh" => {
                    let _ = sender.try_send(());
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });
    receiver
}

fn emit(report: &Report) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    serde_json::to_writer(&mut stdout, report)?;
    stdout.write_all(b"\n")?;
    stdout.flush()
}

fn sample_time() -> io::Result<Duration> {
    let mut time = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: clock_gettime writes to a valid, writable timespec. BOOTTIME is
    // monotonic and includes suspend time on the Linux systems this plugin uses.
    if unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut time) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(Duration::new(time.tv_sec as u64, time.tv_nsec as u32))
}

fn run() -> io::Result<()> {
    let options = match Options::parse() {
        Ok(options) => options,
        Err(error) => {
            return emit(&Report::new(
                Status::unavailable(error),
                Config::default(),
                None,
                0,
            ))
        }
    };
    let requests = options.watch.then(refresh_requests);
    let mut monitor = Monitor::default();
    loop {
        let config = options
            .config
            .as_ref()
            .map_or_else(|| Ok(Config::default()), |path| Config::load(path));
        let status = match &config {
            Ok(_) => probe().unwrap_or_else(Status::unavailable),
            Err(error) => Status::unavailable(error),
        };
        let config = config.unwrap_or_default();
        let interval = Duration::from_secs(config.poll_interval_seconds);
        let max_gap = Duration::from_secs(30.max(config.poll_interval_seconds * 3));
        let wall_time = SystemTime::now();
        let rates = monitor.observe(&status, sample_time()?, max_gap);
        let checked_at_ms = wall_time
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        emit(&Report::new(status, config, rates, checked_at_ms))?;
        let Some(requests) = &requests else {
            return Ok(());
        };
        if let Err(RecvTimeoutError::Disconnected) = requests.recv_timeout(interval) {
            // EOF is normal for `--watch </dev/null`; keep polling, without spinning.
            thread::sleep(interval);
        }
    }
}

fn main() {
    if let Err(error) = run() {
        if error.kind() != io::ErrorKind::BrokenPipe {
            eprintln!("Could not write VPN status: {error}");
            std::process::exit(1);
        }
    }
}
