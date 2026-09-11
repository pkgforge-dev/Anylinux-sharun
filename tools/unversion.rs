//! Strip ELF symbol version requirements from shared objects.
//!
//! ppc64 (big-endian) is ELFv2, but zig's bundled glibc stubs record the old
//! ELFv1 versions (GLIBC_2.3/2.4) that archlinuxpower's glibc does not
//! define, so every preload aborts at launch. Dropping the requirements
//! makes the preloads bind to whatever the running glibc provides.
//! See https://github.com/pkgforge-dev/Anylinux-sharun/issues/11

use std::env;
use std::fs;
use std::process::ExitCode;

use goblin::elf::Elf;
use goblin::elf::dynamic::{DT_VERNEED, DT_VERNEEDNUM, DT_VERSYM};
use goblin::elf::section_header::{SHT_DYNAMIC, SHT_GNU_VERSYM};

// An OS-specific dynamic tag glibc ignores. Used to neutralize the version
// tags in place so the .dynamic array is not truncated.
const IGNORED_TAG: u64 = 0x6fff_f800;

fn main() -> ExitCode {
	let args: Vec<String> = env::args().skip(1).collect();
	if args.is_empty() {
		eprintln!("usage: unversion FILE...");
		return ExitCode::from(2);
	}

	let mut ok = true;
	for path in &args {
		match strip(path) {
			Ok(tags) => println!("{path}: removed {tags} version tag(s)"),
			Err(err) => {
				eprintln!("{path}: {err}");
				ok = false;
			}
		}
	}
	if ok { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

fn strip(path: &str) -> Result<usize, String> {
	let mut data = fs::read(path).map_err(|e| e.to_string())?;

	// Copy what we need out of the parse so we can mutate the bytes after.
	let (little_endian, sections) = {
		let elf = Elf::parse(&data).map_err(|e| format!("not an ELF: {e}"))?;
		if !elf.is_64 {
			return Err("not ELF64".into());
		}
		let sections = elf
			.section_headers
			.iter()
			.map(|sh| (sh.sh_type, sh.sh_offset, sh.sh_size))
			.collect::<Vec<_>>();
		(elf.little_endian, sections)
	};

	let mut tags = 0;
	for (sh_type, sh_offset, sh_size) in sections {
		let start = sh_offset as usize;
		let end = start + sh_size as usize;
		if sh_type == SHT_DYNAMIC {
			let mut off = start;
			while off + 16 <= end {
				let tag = read_u64(&data, off, little_endian);
				if matches!(tag, DT_VERSYM | DT_VERNEED | DT_VERNEEDNUM) {
					write_u64(&mut data, off, IGNORED_TAG, little_endian);
					tags += 1;
				}
				off += 16;
			}
		} else if sh_type == SHT_GNU_VERSYM {
			// Leave the null symbol's index at offset 0 untouched.
			let mut off = start + 2;
			while off + 2 <= end {
				write_u16(&mut data, off, 1, little_endian);
				off += 2;
			}
		}
	}

	fs::write(path, &data).map_err(|e| e.to_string())?;
	Ok(tags)
}

fn read_u64(data: &[u8], off: usize, little_endian: bool) -> u64 {
	let bytes: [u8; 8] = data[off..off + 8].try_into().unwrap();
	if little_endian {
		u64::from_le_bytes(bytes)
	} else {
		u64::from_be_bytes(bytes)
	}
}

fn write_u64(data: &mut [u8], off: usize, value: u64, little_endian: bool) {
	let bytes = if little_endian {
		value.to_le_bytes()
	} else {
		value.to_be_bytes()
	};
	data[off..off + 8].copy_from_slice(&bytes);
}

fn write_u16(data: &mut [u8], off: usize, value: u16, little_endian: bool) {
	let bytes = if little_endian {
		value.to_le_bytes()
	} else {
		value.to_be_bytes()
	};
	data[off..off + 2].copy_from_slice(&bytes);
}
