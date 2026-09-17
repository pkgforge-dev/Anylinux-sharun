//! Minimal ptrace-based kernel compatibility shim.
//!
//! Two classes of problem when running a modern (Arch-built, `--enable-kernel=4.4`)
//! glibc on very old kernels:
//!
//! 1. Instructions. Old kernels lack CPU/XSAVE support, so `XGETBV` (and friends)
//!    raise `#UD` -> `SIGILL`. We catch the signal and emulate the instruction.
//! 2. Syscalls. glibc assumes futex ops added after the target kernel exists
//!    (`FUTEX_WAIT_BITSET`/`FUTEX_WAKE_BITSET`, 2.6.25). On older kernels they
//!    return `ENOSYS` and glibc calls `futex_fatal_error()`. We rewrite those
//!    ops to their older equivalents at syscall-entry.
//!
//! Enable with `SHARUN_OLD_KERNEL_COMPAT=1`; `0` disables. When unset it decides
//! automatically: on for kernels older than 4.0, or when `statx` (4.11) is
//! missing.
//!
//! x86_64 only for now; per-architecture register handling would be required
//! for the others.

#![cfg(target_arch = "x86_64")]

use std::{collections::{HashMap, HashSet}, env, ffi::CStr, process::exit};

use nix::{
	errno::Errno,
	sys::{
		ptrace::{self, AddressType, Options},
		signal::{raise, Signal},
		wait::{waitpid, WaitPidFlag, WaitStatus},
	},
	unistd::{fork, ForkResult, Pid},
	libc,
};

const ENV_ENABLE: &str = "SHARUN_OLD_KERNEL_COMPAT";
const ENV_DEBUG: &str = "SHARUN_OLD_KERNEL_COMPAT_DEBUG";

// futex op encoding
const FUTEX_CMD_MASK: u32 = 0x7f;
const FUTEX_WAIT: u32 = 0;
const FUTEX_WAKE: u32 = 1;
const FUTEX_WAIT_BITSET: u32 = 9;
const FUTEX_WAKE_BITSET: u32 = 10;
const FUTEX_CLOCK_REALTIME: u32 = 256;

fn debug() -> bool {
	matches!(env::var(ENV_DEBUG), Ok(v) if v == "1")
}

fn kernel_lt(want_major: u64, want_minor: u64, want_patch: u64) -> bool {
	let mut uts: libc::utsname = unsafe { std::mem::zeroed() };
	if unsafe { libc::uname(&mut uts) } != 0 {
		return false
	}
	let release = unsafe { CStr::from_ptr(uts.release.as_ptr()) }.to_string_lossy();
	let mut parts = release.split(|c: char| !c.is_ascii_digit());
	let a = parts.next().and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
	let b = parts.next().and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
	let c = parts.next().and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
	(a, b, c) < (want_major, want_minor, want_patch)
}

/// Whether the compatibility tracer should run for this launch.
pub fn enabled() -> bool {
	match env::var(ENV_ENABLE) {
		Ok(v) if v == "0" => false,
		Ok(_) => true,
		Err(_) => needs_compat(),
	}
}

/// Auto mode: run the tracer when the kernel is old enough to plausibly need it.
/// Cheap checks, so modern kernels never pay the ptrace cost.
///
/// Triggered outright for kernels older than 4.0, and otherwise by a missing
/// `statx` (4.11) -- e.g. Qt6 has no fallback when it returns ENOSYS.
///
/// The `statx` probe runs here, in the AppRun process, *before* the application
/// starts: apps may install seccomp filters that reject `statx` (returning
/// ENOSYS), which would be indistinguishable from an old kernel if probed
/// later. That is also why the tracer is only ever set up from AppRun and never
/// from the `bin/*` hardlinks.
fn needs_compat() -> bool {
	if kernel_lt(4, 0, 0) {
		return true
	}
	syscall_missing(libc::SYS_statx)
}

/// True if `nr` is not implemented by this kernel (returns -ENOSYS).
fn syscall_missing(nr: libc::c_long) -> bool {
	let mut stx = [0u8; 256];
	let path = b"/\0";
	let rc = unsafe {
		libc::syscall(
			nr,
			libc::AT_FDCWD,
			path.as_ptr(),
			0i32,
			0u32,
			stx.as_mut_ptr() as *mut libc::c_void,
		)
	};
	rc == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOSYS)
}

/// Run `apprun::run_as_apprun` for `(sharun_dir, bin_dir, exec_args)` under the
/// tracer. Never returns.
pub fn run_apprun_traced(sharun_dir: &str, bin_dir: &str, exec_args: &[String]) -> ! {
	match unsafe { fork() } {
		Ok(ForkResult::Child) => {
			if let Err(err) = ptrace::traceme() {
				eprintln!("kernel-compat: PTRACE_TRACEME failed: {err}");
				exit(1);
			}
			// Let the parent install PTRACE_SETOPTIONS before we run.
			let _ = raise(Signal::SIGSTOP);
			crate::apprun::run_as_apprun(sharun_dir, bin_dir, exec_args)
		},
		Ok(ForkResult::Parent { child }) => supervise(child),
		Err(err) => {
			eprintln!("kernel-compat: fork failed: {err}");
			exit(1);
		},
	}
}

/// Set as many of `wanted` as this kernel supports, one bit at a time.
fn install_options(pid: Pid, wanted: &[Options]) -> Options {
	let mut acc = Options::empty();
	for &opt in wanted {
		if ptrace::setoptions(pid, acc | opt).is_ok() {
			acc |= opt;
		} else {
			let _ = ptrace::setoptions(pid, acc);
			if debug() {
				eprintln!("kernel-compat: option {:?} unsupported, skipped", opt);
			}
		}
	}
	acc
}

fn tracer_options() -> Vec<Options> {
	vec![
		Options::PTRACE_O_TRACESYSGOOD,
		Options::PTRACE_O_TRACEFORK,
		Options::PTRACE_O_TRACEVFORK,
		Options::PTRACE_O_TRACECLONE,
		Options::PTRACE_O_TRACEEXEC,
		Options::PTRACE_O_EXITKILL,
	]
}

struct Tracer {
	child: Pid,
	options: Options,
	configured: HashSet<Pid>,
	in_syscall: HashMap<Pid, bool>,
	last_syscall: HashMap<Pid, u64>,
	statx: HashMap<Pid, StatxState>,
	syscall_mode: bool,
}

struct StatxState {
	statxbuf: u64,
	saved_rsp: u64,
	scratch: u64,
}

impl Tracer {
	fn resume(&self, pid: Pid, sig: Option<Signal>) {
		let res = if self.syscall_mode {
			ptrace::syscall(pid, sig)
		} else {
			ptrace::cont(pid, sig)
		};
		let _ = res;
	}

	fn supervise(&mut self) -> ! {
		if debug() {
			eprintln!("kernel-compat: syscall translation {}", if self.syscall_mode { "on" } else { "off" });
		}
		self.resume(self.child, None);

		loop {
			match waitpid(Pid::from_raw(-1), Some(WaitPidFlag::__WALL)) {
				Ok(WaitStatus::Exited(pid, code)) => {
					if debug() {
						eprintln!("kernel-compat: pid {pid} exited {code}");
					}
					if pid == self.child {
						exit(code)
					}
				},
				Ok(WaitStatus::Signaled(pid, sig, _)) => {
					if debug() {
						eprintln!("kernel-compat: pid {pid} killed by {sig:?}");
					}
					if pid == self.child {
						exit(128 + sig as i32)
					}
				},
				Ok(WaitStatus::Stopped(pid, sig)) => {
					if debug() {
						eprintln!("kernel-compat: pid {pid} stopped: {sig:?}");
					}
					// A newly traced child stops once so we can configure it.
					if pid != self.child && !self.configured.contains(&pid) {
						install_options(pid, &tracer_options());
						self.configured.insert(pid);
						if sig == Signal::SIGSTOP {
							self.resume(pid, None);
							continue;
						}
					}
					if sig == Signal::SIGILL && emulate_sigill(pid) {
						self.resume(pid, None);
					} else {
						self.resume(pid, Some(sig));
					}
				},
				Ok(WaitStatus::PtraceEvent(pid, sig, _event)) => {
					// Children are configured at their first stop below.
					self.resume(pid, Some(sig));
				},
				Ok(WaitStatus::PtraceSyscall(pid)) => {
					self.on_syscall_stop(pid);
					self.resume(pid, None);
				},
				Ok(_) => {},
				Err(Errno::ECHILD) => exit(0),
				Err(err) => {
					if debug() {
						eprintln!("kernel-compat: waitpid error: {err}");
					}
				},
			}
		}
	}

	fn on_syscall_stop(&mut self, pid: Pid) {
		let entering = !*self.in_syscall.get(&pid).unwrap_or(&false);
		self.in_syscall.insert(pid, entering);

		let regs = match ptrace::getregs(pid) {
			Ok(regs) => regs,
			Err(_) => return,
		};

		if entering {
			self.last_syscall.insert(pid, regs.orig_rax);
			if regs.orig_rax == libc::SYS_futex as u64 {
				translate_futex(pid, regs);
			} else if regs.orig_rax == libc::SYS_pipe2 as u64 {
				// pipe2 (2.6.27) -> pipe (ancient). Flags are dropped; callers
				// that need O_NONBLOCK generally set it via fcntl afterwards,
				// and losing O_CLOEXEC only leaks fds into children.
				let mut patched = regs;
				patched.orig_rax = libc::SYS_pipe as u64;
				patched.rax = libc::SYS_pipe as u64;
				let _ = ptrace::setregs(pid, patched);
				if debug() {
					eprintln!("kernel-compat: pipe2 -> pipe (flags {:#x} dropped)", regs.rsi);
				}
			} else if regs.orig_rax == libc::SYS_statx as u64 {
				self.setup_statx(pid, regs);
			}
		} else {
			if self.statx.contains_key(&pid) {
				self.finish_statx(pid, regs);
			}
			if debug() && regs.rax == (-(libc::ENOSYS as i64)) as u64 {
				let nr = self.last_syscall.get(&pid).copied().unwrap_or(0);
				eprintln!("kernel-compat: pid {pid} syscall {nr} -> ENOSYS");
			}
		}
	}

	/// statx (4.11) -> newfstatat (ancient), translating `struct stat` to
	/// `struct statx` on the way back. Qt6 (and others) rely on statx() and
	/// have no fallback when it returns ENOSYS.
	fn setup_statx(&mut self, pid: Pid, mut regs: libc::user_regs_struct) {
		let statxbuf = regs.r8;
		if statxbuf == 0 {
			return
		}
		if debug() {
			eprintln!(
				"kernel-compat: statx path='{}' flags={:#x}",
				read_cstr(pid, regs.rsi),
				regs.rdx
			);
		}
		let saved_rsp = regs.rsp;
		let scratch = (saved_rsp.wrapping_sub(256)) & !0xf;
		// statx(dirfd(rdi), path(rsi), flags(rdx), mask(r10), statxbuf(r8))
		regs.rsp = scratch;
		if regs.rdx & 0x1000 != 0 {
			// AT_EMPTY_PATH (added in 2.6.39) -> fstat(fd, statbuf)
			regs.rsi = scratch;
			regs.rdx = 0;
			regs.orig_rax = libc::SYS_fstat as u64;
			regs.rax = libc::SYS_fstat as u64;
		} else {
			// -> newfstatat(dirfd, path, statbuf(scratch), flags(r10))
			regs.r10 = regs.rdx & 0x900; // AT_SYMLINK_NOFOLLOW|AT_NO_AUTOMOUNT
			regs.rdx = scratch;
			regs.orig_rax = libc::SYS_newfstatat as u64;
			regs.rax = libc::SYS_newfstatat as u64;
		}
		if ptrace::setregs(pid, regs).is_err() {
			return
		}
		self.statx.insert(pid, StatxState { statxbuf, saved_rsp, scratch });
		if debug() {
			eprintln!("kernel-compat: statx -> newfstatat (buf {statxbuf:#x})");
		}
	}

	fn finish_statx(&mut self, pid: Pid, mut regs: libc::user_regs_struct) {
		let Some(state) = self.statx.remove(&pid) else { return };
		let ret = regs.rax as i64;
		if ret == 0 {
			if let Some(stat) = read_struct(pid, state.scratch, 144) {
				let stx = build_statx(&stat);
				let _ = write_struct(pid, state.statxbuf, &stx);
				if debug() {
					eprintln!(
						"kernel-compat: statx ok mode={:#o} size={}",
						rd_u32(&stat, 24),
						rd_u64(&stat, 48)
					);
				}
			}
		} else if debug() {
			eprintln!("kernel-compat: statx -> newfstatat ret={ret} (errno {})", -ret);
		}
		// undo the scratch stack reservation
		regs.rsp = state.saved_rsp;
		let _ = ptrace::setregs(pid, regs);
	}
}

fn translate_futex(pid: Pid, mut regs: libc::user_regs_struct) {
	// x86_64: futex(uaddr, op, val, timeout, uaddr2, val3) ->
	// rdi, rsi, rdx, r10, r8, r9
	let op = regs.rsi as u32;
	let cmd = op & FUTEX_CMD_MASK;
	let realtime = op & FUTEX_CLOCK_REALTIME != 0;
	if debug() {
		eprintln!("kernel-compat: futex entry op={op:#x} cmd={cmd} timeout={:#x}", regs.r10);
	}
	let new_cmd = match cmd {
		FUTEX_WAIT_BITSET => FUTEX_WAIT,
		FUTEX_WAKE_BITSET => FUTEX_WAKE,
		other => other,
	};
	// 2.6.20 predates FUTEX_PRIVATE_FLAG / FUTEX_CLOCK_REALTIME and the bitset
	// ops, so hand the kernel a bare command with no flag bits.
	if new_cmd == op {
		return;
	}
	if new_cmd == FUTEX_WAIT && (cmd == FUTEX_WAIT_BITSET || realtime) {
		// FUTEX_WAIT_BITSET (and CLOCK_REALTIME waits) use an absolute timeout;
		// FUTEX_WAIT wants a relative one.
		convert_timeout_to_relative(pid, &mut regs, realtime);
	}
	regs.rsi = new_cmd as u64;
	if let Err(err) = ptrace::setregs(pid, regs) {
		if debug() {
			eprintln!("kernel-compat: futex setregs failed: {err}");
		}
		return
	}
	if debug() {
		eprintln!("kernel-compat: futex op {op:#x} -> {new_cmd:#x}");
	}
}

fn convert_timeout_to_relative(pid: Pid, regs: &mut libc::user_regs_struct, realtime: bool) {
	// r10 is the timeout pointer (struct timespec, 16 bytes)
	let addr = regs.r10;
	if addr == 0 {
		return
	}
	let read64 = |off: u64| -> Option<i64> {
		ptrace::read(pid, (addr + off) as AddressType).ok().map(|w| w as i64)
	};
	let Some(abs_sec) = read64(0) else { return };
	let Some(abs_nsec) = read64(8) else { return };

	let clock = if realtime {
		libc::CLOCK_REALTIME
	} else {
		libc::CLOCK_MONOTONIC
	};
	let mut now: libc::timespec = unsafe { std::mem::zeroed() };
	if unsafe { libc::clock_gettime(clock, &mut now) } != 0 {
		return
	}
	let mut sec = abs_sec - now.tv_sec;
	let mut nsec = abs_nsec - now.tv_nsec;
	if nsec < 0 {
		sec -= 1;
		nsec += 1_000_000_000;
	}
	if sec < 0 {
		sec = 0;
		nsec = 0;
	}
	let _ = ptrace::write(pid, addr as AddressType, sec);
	let _ = ptrace::write(pid, (addr + 8) as AddressType, nsec);
	if debug() {
		eprintln!("kernel-compat: futex timeout {abs_sec}.{abs_nsec} -> {sec}.{nsec} rel");
	}
}

fn peek_word(pid: Pid, addr: u64) -> Option<u64> {
	ptrace::read(pid, addr as AddressType).ok().map(|w| w as u64)
}

fn read_cstr(pid: Pid, addr: u64) -> String {
	if addr == 0 {
		return String::new()
	}
	let mut out = Vec::new();
	let mut off = 0u64;
	'outer: while out.len() < 256 {
		let Some(word) = peek_word(pid, addr + off) else { break };
		for i in 0..8 {
			let b = ((word >> (8 * i)) & 0xff) as u8;
			if b == 0 {
				break 'outer
			}
			out.push(b);
		}
		off += 8;
	}
	String::from_utf8_lossy(&out).into_owned()
}

#[allow(deprecated)]
fn poke_word(pid: Pid, addr: u64, val: u64) -> bool {
	ptrace::write(pid, addr as AddressType, val as i64).is_ok()
}

fn read_struct(pid: Pid, addr: u64, len: usize) -> Option<Vec<u8>> {
	let mut out = Vec::with_capacity(len + 8);
	let mut off = 0u64;
	while out.len() < len {
		let word = peek_word(pid, addr + off)?;
		out.extend_from_slice(&word.to_le_bytes());
		off += 8;
	}
	out.truncate(len);
	Some(out)
}

fn write_struct(pid: Pid, addr: u64, bytes: &[u8]) -> bool {
	let mut off = 0usize;
	while off < bytes.len() {
		let mut chunk = [0u8; 8];
		let n = (bytes.len() - off).min(8);
		chunk[..n].copy_from_slice(&bytes[off..off + n]);
		if !poke_word(pid, addr + off as u64, u64::from_le_bytes(chunk)) {
			return false
		}
		off += 8;
	}
	true
}

fn rd_u32(b: &[u8], o: usize) -> u32 {
	u32::from_le_bytes(b[o..o + 4].try_into().unwrap_or_default())
}

fn rd_u64(b: &[u8], o: usize) -> u64 {
	u64::from_le_bytes(b[o..o + 8].try_into().unwrap_or_default())
}

fn wr_u32(b: &mut [u8], o: usize, v: u32) {
	b[o..o + 4].copy_from_slice(&v.to_le_bytes());
}

fn wr_u64(b: &mut [u8], o: usize, v: u64) {
	b[o..o + 8].copy_from_slice(&v.to_le_bytes());
}

fn dev_major(dev: u64) -> u32 {
	(((dev >> 8) & 0xfff) | ((dev >> 32) & !0xfffu64)) as u32
}

fn dev_minor(dev: u64) -> u32 {
	((dev & 0xff) | ((dev >> 12) & !0xffu64)) as u32
}

/// `struct stat` (x86_64) -> `struct statx`.
fn build_statx(stat: &[u8]) -> Vec<u8> {
	const STATX_BASIC_STATS: u32 = 0x7ff;
	let mut s = vec![0u8; 256];
	let st_dev = rd_u64(stat, 0);
	let st_ino = rd_u64(stat, 8);
	let st_nlink = rd_u64(stat, 16);
	let st_mode = rd_u32(stat, 24);
	let st_uid = rd_u32(stat, 28);
	let st_gid = rd_u32(stat, 32);
	let st_rdev = rd_u64(stat, 40);
	let st_size = rd_u64(stat, 48);
	let st_blksize = rd_u64(stat, 56);
	let st_blocks = rd_u64(stat, 64);
	let atime_s = rd_u64(stat, 72);
	let atime_ns = rd_u64(stat, 80);
	let mtime_s = rd_u64(stat, 88);
	let mtime_ns = rd_u64(stat, 96);
	let ctime_s = rd_u64(stat, 104);
	let ctime_ns = rd_u64(stat, 112);

	wr_u32(&mut s, 0, STATX_BASIC_STATS);
	wr_u32(&mut s, 4, st_blksize as u32);
	wr_u32(&mut s, 16, st_nlink as u32);
	wr_u32(&mut s, 20, st_uid);
	wr_u32(&mut s, 24, st_gid);
	s[28..30].copy_from_slice(&(st_mode as u16).to_le_bytes());
	wr_u64(&mut s, 32, st_ino);
	wr_u64(&mut s, 40, st_size);
	wr_u64(&mut s, 48, st_blocks);
	wr_u64(&mut s, 64, atime_s);
	wr_u32(&mut s, 72, atime_ns as u32);
	wr_u64(&mut s, 96, ctime_s);
	wr_u32(&mut s, 104, ctime_ns as u32);
	wr_u64(&mut s, 112, mtime_s);
	wr_u32(&mut s, 120, mtime_ns as u32);
	wr_u32(&mut s, 128, dev_major(st_rdev));
	wr_u32(&mut s, 132, dev_minor(st_rdev));
	wr_u32(&mut s, 136, dev_major(st_dev));
	wr_u32(&mut s, 140, dev_minor(st_dev));
	s
}

fn supervise(child: Pid) -> ! {
	// Consume the child's initial SIGSTOP.
	let _ = waitpid(child, None);
	let options = install_options(child, &tracer_options());
	let syscall_mode = options.contains(Options::PTRACE_O_TRACESYSGOOD);
	if debug() {
		eprintln!("kernel-compat: active options: {options:?}");
	}
	let mut tracer = Tracer {
		child,
		options,
		configured: HashSet::from([child]),
		in_syscall: HashMap::new(),
		last_syscall: HashMap::new(),
		statx: HashMap::new(),
		syscall_mode,
	};
	tracer.supervise()
}

#[cfg(target_arch = "x86_64")]
fn module_for(pid: Pid, addr: u64) -> String {
	let maps = match std::fs::read_to_string(format!("/proc/{pid}/maps")) {
		Ok(maps) => maps,
		Err(_) => return "<no maps>".into(),
	};
	for line in maps.lines() {
		let mut cols = line.split_whitespace();
		let range = cols.next().unwrap_or_default();
		let perms = cols.next().unwrap_or_default();
		let _offset = cols.next();
		let _dev = cols.next();
		let _inode = cols.next();
		let path = cols.next().unwrap_or_default();
		let Some((start, end)) = range.split_once('-') else { continue };
		let (Ok(start), Ok(end)) =
			(u64::from_str_radix(start, 16), u64::from_str_radix(end, 16))
		else {
			continue
		};
		if addr >= start && addr < end {
			return format!("{path} [{perms}]");
		}
	}
	"<unknown>".into()
}

#[cfg(target_arch = "x86_64")]
fn emulate_sigill(pid: Pid) -> bool {
	let regs = match ptrace::getregs(pid) {
		Ok(regs) => regs,
		Err(err) => {
			if debug() {
				eprintln!("kernel-compat: getregs failed: {err}");
			}
			return false
		},
	};
	let word = match ptrace::read(pid, regs.rip as AddressType) {
		Ok(word) => word as u64,
		Err(err) => {
			if debug() {
				eprintln!("kernel-compat: peek at 0x{:x} failed: {err}", regs.rip);
			}
			return false
		},
	};
	let opcode = [
		(word & 0xff) as u8,
		((word >> 8) & 0xff) as u8,
		((word >> 16) & 0xff) as u8,
	];
	if debug() {
		eprintln!(
			"kernel-compat: SIGILL at 0x{:x} in {} opcode {:02x} {:02x} {:02x} {:02x} {:02x}",
			regs.rip,
			module_for(pid, regs.rip),
			(word & 0xff) as u8,
			((word >> 8) & 0xff) as u8,
			((word >> 16) & 0xff) as u8,
			((word >> 24) & 0xff) as u8,
			((word >> 32) & 0xff) as u8,
		);
	}
	// XGETBV: 0F 01 D0
	if opcode != [0x0f, 0x01, 0xd0] {
		return false
	}
	// Emulate XGETBV with XCR0 = 0 (no XSAVE state enabled), then skip it.
	let mut patched = regs;
	patched.rax = 0;
	patched.rdx = 0;
	patched.rip = patched.rip.wrapping_add(3);
	if let Err(err) = ptrace::setregs(pid, patched) {
		if debug() {
			eprintln!("kernel-compat: setregs failed: {err}");
		}
		return false
	}
	if debug() {
		eprintln!("kernel-compat: emulated XGETBV (XCR0=0) at 0x{:x}", regs.rip);
	}
	true
}

#[cfg(not(target_arch = "x86_64"))]
fn emulate_sigill(_pid: Pid) -> bool {
	false
}
