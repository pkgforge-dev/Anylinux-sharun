//! `sharun --get-dlopened`: runs a binary with `LD_DEBUG=libs` and prints
//! the libraries the dynamic loader initialized for it, one per line.
//!
//! The binary runs in a process group of its own, so everything it starts is
//! tracked and killed with it, and its stderr is a pipe that sharun reads, so
//! nothing has to be polled in a log file. The trace ends when the first of
//! these happens:
//!
//! - the time given by `--max-time` has passed since the binary was started
//! - no new library was loaded for `--idle-time` (only once one was loaded)
//! - the binary and everything it started have exited
//! - sharun is interrupted

use std::{
	collections::HashSet,
	io::{Read, Write, stdout},
	os::unix::{io::AsRawFd, process::CommandExt},
	process::{Child, Command, Stdio},
	sync::atomic::{AtomicBool, Ordering},
	time::{Duration, Instant},
	thread::sleep,
};

use nix::{
	errno::Errno,
	libc,
	sys::signal::{killpg, sigaction, SaFlags, SigAction, SigHandler, SigSet, Signal},
	unistd::Pid,
};

const DEFAULT_MAX_TIME: Duration = Duration::from_secs(5);
const DEFAULT_IDLE_TIME: Duration = Duration::from_millis(200);

/// Longest line of LD_DEBUG output that is kept while waiting for its end
const MAX_LINE: usize = 65536;

/// How often the exit of the binary is checked, when nothing else wakes us
const TICK: Duration = Duration::from_millis(50);
/// How long a process group gets to exit after SIGTERM before SIGKILL
const TERM_GRACE: Duration = Duration::from_secs(1);

static INTERRUPTED: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(_: libc::c_int) {
	INTERRUPTED.store(true, Ordering::SeqCst)
}

#[derive(Debug, PartialEq)]
struct Options {
	max_time: Duration,
	/// zero turns the idle stop off
	idle_time: Duration,
	binary: String,
	args: Vec<String>,
}

pub fn print_usage() {
	eprintln!("[ Usage ]: sharun --get-dlopened [--max-time SECONDS] [--idle-time MS] [--] BINARY [ARGS]...

Runs BINARY with LD_DEBUG=libs and prints the libraries it loaded, one per line.
Everything BINARY starts is tracked and killed when the trace ends.

	--max-time SECONDS   Longest time to run BINARY for, default {}
	                     (fractions are fine, for example 2.5)
	--idle-time MS       Stop once no new library was loaded for this long
	                     after the first one, default {}, 0 to turn it off",
		DEFAULT_MAX_TIME.as_secs(), DEFAULT_IDLE_TIME.as_millis());
}

fn parse_args(args: &[String]) -> Result<Options, String> {
	let mut max_time = DEFAULT_MAX_TIME;
	let mut idle_time = DEFAULT_IDLE_TIME;
	let mut i = 0;
	let value_of = |flag: &str, i: &mut usize| -> Result<String, String> {
		*i += 1;
		args.get(*i).cloned().ok_or_else(|| format!("{flag} needs a value"))
	};
	while i < args.len() {
		let arg = args[i].as_str();
		let (flag, inline) = match arg.split_once('=') {
			Some((flag, value)) if arg.starts_with("--") => (flag, Some(value.to_string())),
			_ => (arg, None),
		};
		match flag {
			"--max-time" => {
				let value = match inline {
					Some(value) => value,
					None => value_of(flag, &mut i)?,
				};
				let secs: f64 = value.parse().map_err(|_| format!("invalid --max-time: {value}"))?;
				if !secs.is_finite() || secs <= 0.0 || secs > 86400.0 {
					return Err(format!("invalid --max-time: {value}"));
				}
				max_time = Duration::from_secs_f64(secs)
			}
			"--idle-time" => {
				let value = match inline {
					Some(value) => value,
					None => value_of(flag, &mut i)?,
				};
				let ms: u64 = value.parse().map_err(|_| format!("invalid --idle-time: {value}"))?;
				idle_time = Duration::from_millis(ms)
			}
			"--" => {
				i += 1;
				break
			}
			_ => break,
		}
		i += 1
	}
	let binary = args.get(i).cloned().ok_or("missing BINARY")?;
	Ok(Options { max_time, idle_time, binary, args: args[i + 1..].to_vec() })
}

/// The library of a `calling init` line of LD_DEBUG=libs, which looks like
/// `   12345:\tcalling init: /usr/lib/libc.so.6`. The pid and tab are required
/// so a line the program prints itself cannot be taken for one.
fn parse_loaded(line: &str) -> Option<&str> {
	let (pid, rest) = line.trim_start().split_once(":\t")?;
	if pid.is_empty() || !pid.bytes().all(|b| b.is_ascii_digit()) {
		return None;
	}
	let lib = rest.strip_prefix("calling init: ")?.trim_end();
	if lib.is_empty() { None } else { Some(lib) }
}

/// The libraries seen so far, and when the last new one showed up
#[derive(Default)]
struct Loaded {
	libs: Vec<String>,
	seen: HashSet<String>,
	last_new: Option<Instant>,
	partial: Vec<u8>,
}

impl Loaded {
	fn feed(&mut self, chunk: &[u8], now: Instant) {
		self.partial.extend_from_slice(chunk);
		while let Some(end) = self.partial.iter().position(|&b| b == b'\n') {
			let line: Vec<u8> = self.partial.drain(..=end).collect();
			if let Some(lib) = parse_loaded(&String::from_utf8_lossy(&line)) {
				if self.seen.insert(lib.to_string()) {
					self.libs.push(lib.to_string());
					self.last_new = Some(now)
				}
			}
		}
		// a line of LD_DEBUG is a path, anything longer is not one and a
		// program that never prints a newline must not grow this forever
		if self.partial.len() > MAX_LINE {
			self.partial.clear()
		}
	}
}

/// Waits up to `timeout` for the pipe to have something to read
fn wait_readable(fd: i32, timeout: Duration) -> bool {
	let mut pfd = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
	// round up, a zero timeout would turn the wait into a busy loop
	let ms = timeout.as_millis().max(1).min(i32::MAX as u128) as i32;
	let n = unsafe { libc::poll(&mut pfd, 1, ms) };
	n > 0 && pfd.revents != 0
}

/// SIGTERM to the whole group, SIGKILL for what is left after the grace time
fn terminate(child: &mut Child, pgid: Pid) {
	let group_alive = |child: &mut Child| {
		// the leader is a zombie until it is reaped and would keep the group
		let _ = child.try_wait();
		killpg(pgid, None) != Err(Errno::ESRCH)
	};
	let _ = killpg(pgid, Signal::SIGTERM);
	let deadline = Instant::now() + TERM_GRACE;
	while group_alive(child) && Instant::now() < deadline {
		sleep(TICK)
	}
	let _ = killpg(pgid, Signal::SIGKILL);
	let _ = child.wait();
}

fn install_signal_handlers() {
	let action = SigAction::new(SigHandler::Handler(on_signal), SaFlags::empty(), SigSet::empty());
	for signal in [Signal::SIGINT, Signal::SIGTERM, Signal::SIGHUP] {
		let _ = unsafe { sigaction(signal, &action) };
	}
}

/// Runs the trace, returns the exit code of sharun
pub fn run(args: &[String]) -> i32 {
	let opts = match parse_args(args) {
		Ok(opts) => opts,
		Err(err) => {
			eprintln!("{err}\n");
			print_usage();
			return 1
		}
	};

	install_signal_handlers();

	let mut child = match Command::new(&opts.binary)
		.args(&opts.args)
		.env("LD_DEBUG", "libs")
		.env_remove("LD_DEBUG_OUTPUT")
		.stdin(Stdio::null())
		.stdout(Stdio::null())
		.stderr(Stdio::piped())
		.process_group(0)
		.spawn()
	{
		Ok(child) => child,
		Err(err) => {
			eprintln!("Failed to execute {}: {err}", opts.binary);
			return 1
		}
	};
	let started = Instant::now();
	let pgid = Pid::from_raw(child.id() as i32);
	let mut pipe = child.stderr.take().expect("stderr is piped");
	let fd = pipe.as_raw_fd();

	let deadline = started + opts.max_time;
	let mut loaded = Loaded::default();
	let mut chunk = [0u8; 16384];
	let mut interrupted = false;

	loop {
		if INTERRUPTED.load(Ordering::SeqCst) {
			interrupted = true;
			break
		}
		let now = Instant::now();
		if now >= deadline {
			break
		}
		// the time left before the idle stop, when it applies
		let idle_left = match loaded.last_new {
			Some(last) if !opts.idle_time.is_zero() => {
				let left = (last + opts.idle_time).saturating_duration_since(now);
				if left.is_zero() {
					break
				}
				Some(left)
			}
			_ => None,
		};
		let mut wait = (deadline - now).min(TICK);
		if let Some(left) = idle_left {
			wait = wait.min(left)
		}

		if wait_readable(fd, wait) {
			match pipe.read(&mut chunk) {
				// every process that had the pipe is gone
				Ok(0) => break,
				Ok(n) => loaded.feed(&chunk[..n], Instant::now()),
				Err(err) if err.kind() == std::io::ErrorKind::Interrupted => {}
				Err(_) => break,
			}
		}

		// the binary can exit while something it started keeps the pipe
		// open, that something is still being traced, until it is gone too
		if let Ok(Some(_)) = child.try_wait() {
			if killpg(pgid, None) == Err(Errno::ESRCH) {
				break
			}
		}
	}

	// libraries that were loaded in the last moments are still in the pipe
	while wait_readable(fd, Duration::ZERO) {
		match pipe.read(&mut chunk) {
			Ok(n) if n > 0 => loaded.feed(&chunk[..n], Instant::now()),
			_ => break,
		}
	}

	terminate(&mut child, pgid);

	if interrupted {
		return 130
	}

	loaded.libs.sort();
	let mut out = stdout().lock();
	for lib in &loaded.libs {
		if writeln!(out, "{lib}").is_err() {
			return 1
		}
	}
	0
}

#[cfg(test)]
mod tests {
	use super::*;

	fn args(list: &[&str]) -> Vec<String> {
		list.iter().map(|s| s.to_string()).collect()
	}

	#[test]
	fn loaded_line_needs_pid_and_tab() {
		assert_eq!(parse_loaded("      4242:\tcalling init: /usr/lib/libc.so.6"), Some("/usr/lib/libc.so.6"));
		assert_eq!(parse_loaded("4242:\tcalling init: /a b/lib x.so\n"), Some("/a b/lib x.so"));
		assert_eq!(parse_loaded("      4242:\tcalling fini: /usr/lib/libc.so.6 [0]"), None);
		assert_eq!(parse_loaded("      4242:\tinitialize program: ./app"), None);
		assert_eq!(parse_loaded("calling init: /usr/lib/libc.so.6"), None);
		assert_eq!(parse_loaded("app: calling init: /x.so"), None);
		assert_eq!(parse_loaded("x:\tcalling init: /x.so"), None);
		assert_eq!(parse_loaded("12:\tcalling init: "), None);
	}

	#[test]
	fn lines_split_across_reads_are_joined() {
		let mut loaded = Loaded::default();
		let now = Instant::now();
		loaded.feed(b"  1:\tcalling init: /lib/a.so\n  1:\tcalling i", now);
		assert_eq!(loaded.libs, ["/lib/a.so"]);
		loaded.feed(b"nit: /lib/b.so\n  1:\tcalling init: /lib/a.so\n", now);
		assert_eq!(loaded.libs, ["/lib/a.so", "/lib/b.so"]);
	}

	#[test]
	fn endless_line_is_dropped() {
		let mut loaded = Loaded::default();
		let now = Instant::now();
		let junk = vec![b'x'; MAX_LINE];
		loaded.feed(&junk, now);
		loaded.feed(&junk, now);
		assert!(loaded.partial.len() <= MAX_LINE);
		loaded.feed(b"\n  1:\tcalling init: /lib/a.so\n", now);
		assert_eq!(loaded.libs, ["/lib/a.so"]);
	}

	#[test]
	fn defaults_and_binary_arguments() {
		let opts = parse_args(&args(&["/bin/app", "--max-time", "9", "-x"])).unwrap();
		assert_eq!(opts.max_time, DEFAULT_MAX_TIME);
		assert_eq!(opts.idle_time, DEFAULT_IDLE_TIME);
		assert_eq!(opts.binary, "/bin/app");
		assert_eq!(opts.args, ["--max-time", "9", "-x"]);
	}

	#[test]
	fn options_in_both_spellings() {
		let opts = parse_args(&args(&["--max-time", "2.5", "--idle-time=0", "app", "a"])).unwrap();
		assert_eq!(opts.max_time, Duration::from_millis(2500));
		assert_eq!(opts.idle_time, Duration::ZERO);
		assert_eq!(opts.binary, "app");
		assert_eq!(opts.args, ["a"]);
		let opts = parse_args(&args(&["--max-time=1", "--", "--odd-name"])).unwrap();
		assert_eq!(opts.binary, "--odd-name");
	}

	#[test]
	fn bad_arguments_are_rejected() {
		for bad in [
			&["--max-time", "0", "app"][..], &["--max-time", "-1", "app"], &["--max-time", "abc", "app"],
			&["--max-time", "inf", "app"], &["--max-time", "NaN", "app"], &["--max-time"],
			&["--idle-time", "x", "app"], &["--idle-time", "-5", "app"], &[], &["--max-time", "3"],
		] {
			assert!(parse_args(&args(bad)).is_err(), "{bad:?} must be rejected");
		}
	}
}
