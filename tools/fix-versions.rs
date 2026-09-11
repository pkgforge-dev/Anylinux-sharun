//! Rewrite ELFv1-only glibc symbol version requirements to the ELFv2
//! baseline (`GLIBC_2.17`).
//!
//! ppc64 (big-endian) is ELFv2, but zig's bundled glibc stubs record the old
//! ELFv1 versions (GLIBC_2.3/2.4) that archlinuxpower's glibc does not
//! define, so every preload aborts at launch. Bumping them to GLIBC_2.17
//! keeps the preloads versioned while loading on the bundled glibc.
//! See https://github.com/pkgforge-dev/Anylinux-sharun/issues/11

use std::env;
use std::fs;
use std::process::ExitCode;

use goblin::elf::Elf;
use goblin::elf::dynamic::{DT_HASH, DT_STRTAB, DT_STRSZ};
use goblin::elf::section_header::{SHT_DYNAMIC, SHT_GNU_VERNEED, SHT_HASH, SHT_STRTAB};

const NEW_VERSION: &[u8] = b"GLIBC_2.17\0";
const NEW_VERSION_NUM: (u32, u32, u32) = (2, 17, 0);
const IGNORED_TAG: u64 = 0x6fff_f800;

#[derive(Clone, Copy)]
struct Section {
	sh_type: u32,
	addr: u64,
	offset: u64,
	size: u64,
	link: u32,
}

fn main() -> ExitCode {
	let args: Vec<String> = env::args().skip(1).collect();
	if args.is_empty() {
		eprintln!("usage: fix-versions FILE...");
		return ExitCode::from(2);
	}

	let mut ok = true;
	for path in &args {
		match fix(path) {
			Ok(0) => println!("{path}: no ELFv1 versions"),
			Ok(n) => println!("{path}: bumped {n} version(s) to GLIBC_2.17"),
			Err(err) => {
				eprintln!("{path}: {err}");
				ok = false;
			}
		}
	}
	if ok { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

fn fix(path: &str) -> Result<usize, String> {
	let mut data = fs::read(path).map_err(|e| e.to_string())?;
	let endian;

	// Copy everything we need out of the parse so the bytes can be mutated.
	let (sections, shoff, shentsize, dyn_off, dyn_size) = {
		let elf = Elf::parse(&data).map_err(|e| format!("not an ELF: {e}"))?;
		if !elf.is_64 {
			return Err("not ELF64".into());
		}
		endian = elf.little_endian;
		let sections = elf
			.section_headers
			.iter()
			.map(|sh| Section {
				sh_type: sh.sh_type,
				addr: sh.sh_addr,
				offset: sh.sh_offset,
				size: sh.sh_size,
				link: sh.sh_link,
			})
			.collect::<Vec<_>>();
		let (dyn_off, dyn_size) = {
			let dynamic = sections
				.iter()
				.find(|s| s.sh_type == SHT_DYNAMIC)
				.ok_or("no .dynamic")?;
			(dynamic.offset, dynamic.size)
		};
		(
			sections,
			elf.header.e_shoff,
			elf.header.e_shentsize as u64,
			dyn_off,
			dyn_size,
		)
	};

	let mut strtab_off = None;
	let mut strsz = 0u64;
	let mut hash_off = None;
	for i in 0..dyn_size / 16 {
		let off = (dyn_off + i * 16) as usize;
		match read_u64(&data, off, endian) {
			DT_STRTAB => strtab_off = Some(read_u64(&data, off + 8, endian)),
			DT_STRSZ => strsz = read_u64(&data, off + 8, endian),
			DT_HASH => hash_off = Some(read_u64(&data, off + 8, endian)),
			_ => {}
		}
	}
	let strtab_off = strtab_off.ok_or("no DT_STRTAB")?;

	// Find the ELFv1-only requirements before touching anything.
	let mut bad = Vec::new();
	for s in &sections {
		if s.sh_type != SHT_GNU_VERNEED {
			continue;
		}
		let names = sections[s.link as usize].offset;
		let mut p = s.offset;
		loop {
			let cnt = read_u16(&data, p as usize + 2, endian);
			let mut a = p + read_u32(&data, p as usize + 8, endian) as u64;
			for _ in 0..cnt {
				let name_off = read_u32(&data, a as usize + 8, endian) as u64;
				let next = read_u32(&data, a as usize + 12, endian) as u64;
				let name = read_str(&data, names + name_off);
				if is_elfv1_only(&name) {
					bad.push(a);
				}
				if next == 0 {
					break;
				}
				a += next;
			}
			let next = read_u32(&data, p as usize + 12, endian) as u64;
			if next == 0 {
				break;
			}
			p += next;
		}
	}
	if bad.is_empty() {
		return Ok(0);
	}

	let hash_off = hash_off.ok_or("no DT_HASH (nowhere to put the new string)")?;
	sections
		.iter()
		.find(|s| s.sh_type == SHT_STRTAB && s.offset == strtab_off)
		.ok_or("no .dynstr")?;
	sections
		.iter()
		.find(|s| s.sh_type == SHT_HASH && s.offset == hash_off)
		.ok_or("no .hash")?;
	if !sections.iter().any(|s| s.sh_type == 0x6fff_fff6) {
		return Err("no .gnu.hash, cannot drop .hash".into());
	}
	if sections
		.iter()
		.any(|s| s.sh_type != 0 && s.offset > hash_off && s.offset < strtab_off)
	{
		return Err("unrelated section between .hash and .dynstr".into());
	}
	if strtab_off + strsz - hash_off < strsz + NEW_VERSION.len() as u64 {
		return Err("not enough room to grow .dynstr".into());
	}

	// Relocate .dynstr over .hash and append the new version string.
	let new_off = hash_off;
	let new_size = strsz + NEW_VERSION.len() as u64;
	let old = data[strtab_off as usize..(strtab_off + strsz) as usize].to_vec();
	data[new_off as usize..new_off as usize + old.len()].copy_from_slice(&old);
	data[(new_off + strsz) as usize..(new_off + new_size) as usize].copy_from_slice(NEW_VERSION);

	for i in 0..dyn_size / 16 {
		let off = (dyn_off + i * 16) as usize;
		match read_u64(&data, off, endian) {
			DT_STRTAB => write_u64(&mut data, off + 8, new_off, endian),
			DT_STRSZ => write_u64(&mut data, off + 8, new_size, endian),
			DT_HASH => write_u64(&mut data, off, IGNORED_TAG, endian),
			_ => {}
		}
	}

	for (i, s) in sections.iter().enumerate() {
		let off = (shoff + i as u64 * shentsize) as usize;
		if s.sh_type == SHT_STRTAB && s.offset == strtab_off {
			write_u64(&mut data, off + 0x10, new_off + (s.addr - s.offset), endian);
			write_u64(&mut data, off + 0x18, new_off, endian);
			write_u64(&mut data, off + 0x20, new_size, endian);
		} else if s.sh_type == SHT_HASH && s.offset == hash_off {
			write_u32(&mut data, off + 0x04, 0, endian);
		}
	}

	// Point every ELFv1-only requirement at the appended string.
	let name = &NEW_VERSION[..NEW_VERSION.len() - 1];
	for a in &bad {
		write_u32(&mut data, *a as usize, elf_hash(name), endian);
		write_u32(&mut data, *a as usize + 8, strsz as u32, endian);
	}

	fs::write(path, &data).map_err(|e| e.to_string())?;
	Ok(bad.len())
}

fn is_elfv1_only(name: &str) -> bool {
	let Some(rest) = name.strip_prefix("GLIBC_") else {
		return false;
	};
	let mut nums = [0u32; 3];
	let mut n = 0;
	for part in rest.split('.') {
		if n == 3 {
			break;
		}
		match part.parse::<u32>() {
			Ok(v) => nums[n] = v,
			Err(_) => return false,
		}
		n += 1;
	}
	while n < 3 {
		nums[n] = 0;
		n += 1;
	}
	(nums[0], nums[1], nums[2]) < NEW_VERSION_NUM
}

fn elf_hash(name: &[u8]) -> u32 {
	let mut h: u32 = 0;
	for &c in name {
		h = (h << 4).wrapping_add(c as u32);
		let g = h & 0xf000_0000;
		if g != 0 {
			h ^= g >> 24;
			h &= !g;
		}
	}
	h
}

fn read_str(data: &[u8], off: u64) -> String {
	let start = off as usize;
	let end = data[start..].iter().position(|&b| b == 0).map(|i| start + i).unwrap();
	String::from_utf8_lossy(&data[start..end]).into_owned()
}

fn read_u16(data: &[u8], off: usize, le: bool) -> u16 {
	let b: [u8; 2] = data[off..off + 2].try_into().unwrap();
	if le { u16::from_le_bytes(b) } else { u16::from_be_bytes(b) }
}

fn read_u32(data: &[u8], off: usize, le: bool) -> u32 {
	let b: [u8; 4] = data[off..off + 4].try_into().unwrap();
	if le { u32::from_le_bytes(b) } else { u32::from_be_bytes(b) }
}

fn read_u64(data: &[u8], off: usize, le: bool) -> u64 {
	let b: [u8; 8] = data[off..off + 8].try_into().unwrap();
	if le { u64::from_le_bytes(b) } else { u64::from_be_bytes(b) }
}

fn write_u32(data: &mut [u8], off: usize, v: u32, le: bool) {
	let b = if le { v.to_le_bytes() } else { v.to_be_bytes() };
	data[off..off + 4].copy_from_slice(&b);
}

fn write_u64(data: &mut [u8], off: usize, v: u64, le: bool) {
	let b = if le { v.to_le_bytes() } else { v.to_be_bytes() };
	data[off..off + 8].copy_from_slice(&b);
}
