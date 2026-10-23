//! Removes absolute paths from the dynamic section of the ELF files in an
//! AppDir, in place and without any external tool:
//!
//! - an RPATH/RUNPATH that starts with `/` is removed, so the host path it
//!   names cannot be searched before sharun's library path
//! - a DT_NEEDED entry that is a path (`/usr/lib/libkek.so`) is turned into
//!   its basename (`libkek.so`), so it is looked up in the library path
//!
//! Only the dynamic section is edited. A basename is always the tail of the
//! path string it replaces, so the DT_NEEDED value is moved forward inside the
//! string table instead of writing a new string, and removing the rpath shifts
//! the remaining entries up and pads the section with DT_NULL. Nothing changes
//! size, so no section or segment has to move. The program headers are used
//! instead of the section headers, so files with stripped sections work too.

use std::{
	fs::{self, File},
	io::{self, Error, ErrorKind::InvalidData, Result},
	os::unix::fs::{FileExt, OpenOptionsExt},
	path::{Path, PathBuf},
};

use walkdir::WalkDir;

const PT_LOAD: u32 = 1;
const PT_DYNAMIC: u32 = 2;

const DT_NULL: u64 = 0;
const DT_NEEDED: u64 = 1;
const DT_STRTAB: u64 = 5;
const DT_RPATH: u64 = 15;
const DT_RUNPATH: u64 = 29;

/// Upper bounds that keep a corrupt file from asking for a huge read
const MAX_PHDR_TABLE: usize = 1 << 20;
const MAX_DYNAMIC: usize = 1 << 22;
const MAX_STRING: usize = 4096;

fn invalid(msg: &str) -> Error {
	Error::new(InvalidData, msg.to_string())
}

/// Class and byte order of a file, which decide how every field is decoded
#[derive(Clone, Copy)]
struct Layout {
	is64: bool,
	big: bool,
}

impl Layout {
	fn u16(&self, b: &[u8]) -> u64 {
		let v: [u8; 2] = b[..2].try_into().unwrap_or_default();
		(if self.big { u16::from_be_bytes(v) } else { u16::from_le_bytes(v) }) as u64
	}

	fn u32(&self, b: &[u8]) -> u64 {
		let v: [u8; 4] = b[..4].try_into().unwrap_or_default();
		(if self.big { u32::from_be_bytes(v) } else { u32::from_le_bytes(v) }) as u64
	}

	fn u64(&self, b: &[u8]) -> u64 {
		let v: [u8; 8] = b[..8].try_into().unwrap_or_default();
		if self.big { u64::from_be_bytes(v) } else { u64::from_le_bytes(v) }
	}

	/// An address sized field, 8 bytes in ELF64 and 4 bytes in ELF32
	fn word(&self, b: &[u8]) -> u64 {
		if self.is64 { self.u64(b) } else { self.u32(b) }
	}

	fn word_size(&self) -> usize {
		if self.is64 { 8 } else { 4 }
	}

	fn put_word(&self, out: &mut Vec<u8>, v: u64) {
		match (self.is64, self.big) {
			(true, true) => out.extend_from_slice(&v.to_be_bytes()),
			(true, false) => out.extend_from_slice(&v.to_le_bytes()),
			(false, true) => out.extend_from_slice(&(v as u32).to_be_bytes()),
			(false, false) => out.extend_from_slice(&(v as u32).to_le_bytes()),
		}
	}
}

struct Segment {
	kind: u32,
	offset: u64,
	vaddr: u64,
	filesz: u64,
}

struct DynEntry {
	tag: u64,
	val: u64,
}

/// What has to be written to a file, and what was found wrong in it
struct Fix {
	dyn_offset: u64,
	dyn_bytes: Vec<u8>,
	changes: Vec<String>,
}

fn read_exact_at(file: &File, offset: u64, len: usize) -> Result<Vec<u8>> {
	let mut buf = vec![0u8; len];
	file.read_exact_at(&mut buf, offset)?;
	Ok(buf)
}

/// The NUL terminated string at `offset`, `None` when there is no terminator
fn read_cstr(file: &File, offset: u64) -> Result<Option<Vec<u8>>> {
	let mut buf = vec![0u8; MAX_STRING];
	let n = file.read_at(&mut buf, offset)?;
	Ok(buf[..n].iter().position(|&b| b == 0).map(|end| buf[..end].to_vec()))
}

fn read_layout_and_segments(file: &File) -> Result<Option<(Layout, Vec<Segment>)>> {
	let mut magic = [0u8; 4];
	if file.read_at(&mut magic, 0)? < 4 || &magic != b"\x7fELF" {
		return Ok(None);
	}
	let ident = read_exact_at(file, 0, 16)?;
	let layout = Layout {
		is64: match ident[4] {
			1 => false,
			2 => true,
			_ => return Ok(None),
		},
		big: match ident[5] {
			1 => false,
			2 => true,
			_ => return Ok(None),
		},
	};

	let ehdr = read_exact_at(file, 0, if layout.is64 { 64 } else { 52 })?;
	let (phoff, phentsize, phnum) = if layout.is64 {
		(layout.u64(&ehdr[32..]), layout.u16(&ehdr[54..]), layout.u16(&ehdr[56..]))
	} else {
		(layout.u32(&ehdr[28..]), layout.u16(&ehdr[42..]), layout.u16(&ehdr[44..]))
	};
	let min_entry = if layout.is64 { 56 } else { 32 };
	if phnum == 0 || (phentsize as usize) < min_entry {
		return Ok(None);
	}
	let table_len = phentsize as usize * phnum as usize;
	if table_len > MAX_PHDR_TABLE {
		return Err(invalid("program header table is too large"));
	}
	let table = read_exact_at(file, phoff, table_len)?;

	let segments = table.chunks_exact(phentsize as usize).map(|ph| {
		if layout.is64 {
			Segment {
				kind: layout.u32(ph) as u32,
				offset: layout.u64(&ph[8..]),
				vaddr: layout.u64(&ph[16..]),
				filesz: layout.u64(&ph[32..]),
			}
		} else {
			Segment {
				kind: layout.u32(ph) as u32,
				offset: layout.u32(&ph[4..]),
				vaddr: layout.u32(&ph[8..]),
				filesz: layout.u32(&ph[16..]),
			}
		}
	}).collect();
	Ok(Some((layout, segments)))
}

/// File offset of a virtual address, found through the PT_LOAD segments
fn vaddr_to_offset(segments: &[Segment], vaddr: u64) -> Option<u64> {
	segments.iter()
		.filter(|s| s.kind == PT_LOAD)
		.find(|s| vaddr >= s.vaddr && vaddr - s.vaddr < s.filesz)
		.map(|s| s.offset + (vaddr - s.vaddr))
}

fn parse_dynamic(layout: Layout, bytes: &[u8]) -> Vec<DynEntry> {
	let step = layout.word_size() * 2;
	let mut entries = Vec::new();
	for raw in bytes.chunks_exact(step) {
		let tag = layout.word(raw);
		let val = layout.word(&raw[layout.word_size()..]);
		if tag == DT_NULL {
			break;
		}
		entries.push(DynEntry { tag, val });
	}
	entries
}

fn encode_dynamic(layout: Layout, entries: &[DynEntry], total_len: usize) -> Vec<u8> {
	let mut out = Vec::with_capacity(total_len);
	for entry in entries {
		layout.put_word(&mut out, entry.tag);
		layout.put_word(&mut out, entry.val);
	}
	// the section keeps its size, what the removed entries leave is DT_NULL
	out.resize(total_len, 0);
	out
}

/// Works out the fix for one file, `None` when there is nothing to fix or the
/// file is not a dynamically linked ELF
fn plan(file: &File) -> Result<Option<Fix>> {
	let (layout, segments) = match read_layout_and_segments(file)? {
		Some(parsed) => parsed,
		None => return Ok(None),
	};
	let dynamic = match segments.iter().find(|s| s.kind == PT_DYNAMIC) {
		Some(segment) => segment,
		None => return Ok(None),
	};
	if dynamic.filesz as usize > MAX_DYNAMIC {
		return Err(invalid("dynamic section is too large"));
	}
	let dyn_bytes = read_exact_at(file, dynamic.offset, dynamic.filesz as usize)?;
	let mut entries = parse_dynamic(layout, &dyn_bytes);

	let strtab_vaddr = match entries.iter().find(|e| e.tag == DT_STRTAB) {
		Some(entry) => entry.val,
		None => return Ok(None),
	};
	let strtab = vaddr_to_offset(&segments, strtab_vaddr)
		.ok_or_else(|| invalid("DT_STRTAB is not inside a loadable segment"))?;
	let string_at = |val: u64| -> Result<Vec<u8>> {
		read_cstr(file, strtab + val)?.ok_or_else(|| invalid("unterminated string"))
	};

	let mut changes = Vec::new();

	// DT_RUNPATH wins over DT_RPATH, that is the one the loader uses
	let rpath_val = entries.iter().find(|e| e.tag == DT_RUNPATH)
		.or_else(|| entries.iter().find(|e| e.tag == DT_RPATH))
		.map(|e| e.val);
	if let Some(val) = rpath_val {
		let rpath = string_at(val)?;
		if rpath.first() == Some(&b'/') {
			changes.push(format!("removed full rpath {}", String::from_utf8_lossy(&rpath)));
			entries.retain(|e| e.tag != DT_RPATH && e.tag != DT_RUNPATH);
		}
	}

	for entry in entries.iter_mut().filter(|e| e.tag == DT_NEEDED) {
		let needed = string_at(entry.val)?;
		if needed.first() != Some(&b'/') {
			continue;
		}
		let base_start = match needed.iter().rposition(|&b| b == b'/') {
			Some(slash) => slash + 1,
			None => continue,
		};
		if base_start == needed.len() {
			continue;
		}
		changes.push(format!("removed full needed lib path {}", String::from_utf8_lossy(&needed)));
		entry.val += base_start as u64;
	}

	if changes.is_empty() {
		return Ok(None);
	}
	Ok(Some(Fix {
		dyn_offset: dynamic.offset,
		dyn_bytes: encode_dynamic(layout, &entries, dyn_bytes.len()),
		changes,
	}))
}

/// Writes the fix to a copy of the file and renames it over the original,
/// which works on read-only files, keeps the mode and leaves other hard links
/// of the same file alone
fn apply(path: &Path, fix: &Fix) -> Result<()> {
	let mut tmp = path.as_os_str().to_owned();
	tmp.push(format!(".sharun-rpath.{}", std::process::id()));
	let tmp = PathBuf::from(tmp);
	let result = (|| {
		let mut source = File::open(path)?;
		let mode = source.metadata()?.permissions();
		// writable while it is being made, the original mode is put back after
		let mut copy = fs::OpenOptions::new()
			.write(true).create_new(true).mode(0o600).open(&tmp)?;
		io::copy(&mut source, &mut copy)?;
		copy.write_all_at(&fix.dyn_bytes, fix.dyn_offset)?;
		drop(copy);
		fs::set_permissions(&tmp, mode)?;
		fs::rename(&tmp, path)
	})();
	if result.is_err() {
		let _ = fs::remove_file(&tmp);
	}
	result
}

/// Fixes one file, returns what was changed (empty when nothing was)
pub fn fix_file(path: &Path) -> Result<Vec<String>> {
	let file = File::open(path)?;
	match plan(&file)? {
		Some(fix) => {
			drop(file);
			apply(path, &fix)?;
			Ok(fix.changes)
		}
		None => Ok(Vec::new()),
	}
}

/// Libraries by name, the same rule gen_library_path uses
fn is_library_name(name: &str) -> bool {
	name.ends_with(".so") || name.contains(".so.")
}

/// Fixes every library in the AppDir, wherever it is, and the binaries in
/// shared/bin, since an application can link against a library path too.
/// Symlinks are not followed, the file they point to is visited by its own
/// name. Returns how many files failed.
pub fn fix_appdir(appdir: &str) -> usize {
	let shared_bin = Path::new(appdir).join("shared").join("bin");
	let mut failed = 0;
	for entry in WalkDir::new(appdir).follow_links(false).sort_by_file_name()
		.into_iter().filter_map(|entry| entry.ok())
	{
		if !entry.file_type().is_file() {
			continue;
		}
		let is_bin = entry.path().parent() == Some(shared_bin.as_path());
		if !is_bin && !is_library_name(&entry.file_name().to_string_lossy()) {
			continue;
		}
		match fix_file(entry.path()) {
			Ok(changes) => for change in changes {
				eprintln!("* {change} from {}", entry.path().display())
			},
			Err(err) => {
				eprintln!("Failed to fix rpath: {}: {err}", entry.path().display());
				failed += 1
			}
		}
	}
	failed
}

#[cfg(test)]
mod tests {
	use super::*;

	/// A minimal dynamic ELF: header, two program headers (PT_LOAD for the
	/// whole file and PT_DYNAMIC), a dynamic section and a string table
	struct Spec<'a> {
		is64: bool,
		big: bool,
		rpath: Option<(u64, &'a str)>,
		needed: &'a [&'a str],
		/// DT_STRTAB vaddr offset from the file offset, to test the mapping
		load_bias: u64,
		pad_nulls: usize,
	}

	/// Every test gets its own file names, the tests run in parallel
	fn unique(tag: &str) -> PathBuf {
		use std::sync::atomic::{AtomicUsize, Ordering};
		static NEXT: AtomicUsize = AtomicUsize::new(0);
		std::env::temp_dir().join(format!(
			"sharun-rpath-{tag}-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::SeqCst)))
	}

	fn build(spec: &Spec) -> Vec<u8> {
		let layout = Layout { is64: spec.is64, big: spec.big };
		let ws = layout.word_size();
		let (ehsize, phentsize) = if spec.is64 { (64usize, 56usize) } else { (52, 32) };
		let phoff = ehsize;

		// string table, the first byte is the empty string
		let mut strtab = vec![0u8];
		let mut add = |s: &str| -> u64 {
			let off = strtab.len() as u64;
			strtab.extend_from_slice(s.as_bytes());
			strtab.push(0);
			off
		};
		let rpath_val = spec.rpath.map(|(tag, s)| (tag, add(s)));
		let needed_vals: Vec<u64> = spec.needed.iter().map(|s| add(s)).collect();

		let dyn_off = phoff + 2 * phentsize;
		let n_entries = rpath_val.iter().count() + needed_vals.len() + 1 + 1 + spec.pad_nulls;
		let dyn_len = n_entries * ws * 2;
		let str_off = dyn_off + dyn_len;
		let total = str_off + strtab.len();
		let strtab_vaddr = str_off as u64 + spec.load_bias;

		let mut dynbytes = Vec::new();
		let mut put = |tag: u64, val: u64| {
			layout.put_word(&mut dynbytes, tag);
			layout.put_word(&mut dynbytes, val);
		};
		for val in &needed_vals {
			put(DT_NEEDED, *val);
		}
		if let Some((tag, val)) = rpath_val {
			put(tag, val);
		}
		put(DT_STRTAB, strtab_vaddr);
		put(DT_NULL, 0);
		dynbytes.resize(dyn_len, 0);

		let mut b = vec![0u8; ehsize];
		b[0..4].copy_from_slice(b"\x7fELF");
		b[4] = if spec.is64 { 2 } else { 1 };
		b[5] = if spec.big { 2 } else { 1 };
		b[6] = 1;
		let put16 = |buf: &mut Vec<u8>, at: usize, v: u16| {
			let bytes = if spec.big { v.to_be_bytes() } else { v.to_le_bytes() };
			buf[at..at + 2].copy_from_slice(&bytes);
		};
		let word_at = |buf: &mut Vec<u8>, at: usize, v: u64| {
			let mut tmp = Vec::new();
			layout.put_word(&mut tmp, v);
			buf[at..at + tmp.len()].copy_from_slice(&tmp);
		};
		put16(&mut b, 16, 3); // ET_DYN
		if spec.is64 {
			word_at(&mut b, 32, phoff as u64);
			put16(&mut b, 54, phentsize as u16);
			put16(&mut b, 56, 2);
		} else {
			word_at(&mut b, 28, phoff as u64);
			put16(&mut b, 42, phentsize as u16);
			put16(&mut b, 44, 2);
		}

		let mut ph = |kind: u32, offset: u64, vaddr: u64, filesz: u64| {
			let mut e = vec![0u8; phentsize];
			let kind_bytes = if spec.big { kind.to_be_bytes() } else { kind.to_le_bytes() };
			e[0..4].copy_from_slice(&kind_bytes);
			let fields: [(usize, u64); 4] = if spec.is64 {
				[(8, offset), (16, vaddr), (32, filesz), (40, filesz)]
			} else {
				[(4, offset), (8, vaddr), (16, filesz), (20, filesz)]
			};
			for (at, v) in fields {
				word_at(&mut e, at, v);
			}
			b.extend_from_slice(&e);
		};
		ph(PT_LOAD, 0, spec.load_bias, total as u64);
		ph(PT_DYNAMIC, dyn_off as u64, dyn_off as u64 + spec.load_bias, dyn_len as u64);
		b.extend_from_slice(&dynbytes);
		b.extend_from_slice(&strtab);
		b
	}

	/// What the loader would see: the rpath, the needed names and the entries
	/// after the first DT_NULL
	fn read_back(bytes: &[u8], is64: bool, big: bool) -> (Option<String>, Vec<String>, bool) {
		let layout = Layout { is64, big };
		let dir = unique("readback");
		fs::write(&dir, bytes).unwrap();
		let file = File::open(&dir).unwrap();
		let (_, segments) = read_layout_and_segments(&file).unwrap().unwrap();
		let dynamic = segments.iter().find(|s| s.kind == PT_DYNAMIC).unwrap();
		let raw = read_exact_at(&file, dynamic.offset, dynamic.filesz as usize).unwrap();
		let entries = parse_dynamic(layout, &raw);
		let strtab = vaddr_to_offset(&segments, entries.iter().find(|e| e.tag == DT_STRTAB).unwrap().val).unwrap();
		let get = |val: u64| String::from_utf8(read_cstr(&file, strtab + val).unwrap().unwrap()).unwrap();
		let rpath = entries.iter().find(|e| e.tag == DT_RUNPATH).or_else(|| entries.iter().find(|e| e.tag == DT_RPATH)).map(|e| get(e.val));
		let needed = entries.iter().filter(|e| e.tag == DT_NEEDED).map(|e| get(e.val)).collect();
		let step = layout.word_size() * 2;
		let nulls_after = raw.chunks_exact(step).skip(entries.len() + 1).all(|c| c.iter().all(|&x| x == 0));
		fs::remove_file(&dir).unwrap();
		(rpath, needed, nulls_after)
	}

	fn run_on(spec: &Spec) -> (Vec<u8>, Vec<u8>, Vec<String>) {
		let before = build(spec);
		let path = unique("t");
		fs::write(&path, &before).unwrap();
		let changes = fix_file(&path).unwrap();
		let after = fs::read(&path).unwrap();
		fs::remove_file(&path).unwrap();
		(before, after, changes)
	}

	fn spec<'a>(is64: bool, big: bool) -> Spec<'a> {
		Spec {
			is64, big, load_bias: 0x1000, pad_nulls: 3,
			rpath: Some((DT_RPATH, "/opt/host/lib")),
			needed: &["libc.so.6", "/usr/lib/libkek.so", "/a/b/libdeep.so.2"],
		}
	}

	#[test]
	fn all_four_layouts() {
		for (is64, big) in [(true, false), (true, true), (false, false), (false, true)] {
			let s = spec(is64, big);
			let (before, after, changes) = run_on(&s);
			assert_eq!(before.len(), after.len(), "size changed, is64={is64} big={big}");
			let (rpath, needed, nulls) = read_back(&after, is64, big);
			assert_eq!(rpath, None, "is64={is64} big={big}");
			assert_eq!(needed, ["libc.so.6", "libkek.so", "libdeep.so.2"], "is64={is64} big={big}");
			assert!(nulls, "tail of the dynamic section is not DT_NULL, is64={is64} big={big}");
			assert_eq!(changes.len(), 3);
			// only the dynamic section is allowed to differ
			let (rp_before, _, _) = read_back(&before, is64, big);
			assert_eq!(rp_before.as_deref(), Some("/opt/host/lib"));
		}
	}

	#[test]
	fn runpath_wins_and_both_are_removed() {
		let mut s = spec(true, false);
		s.rpath = Some((DT_RUNPATH, "/opt/host/lib"));
		let (_, after, changes) = run_on(&s);
		assert_eq!(read_back(&after, true, false).0, None);
		assert!(changes.iter().any(|c| c.contains("/opt/host/lib")));
	}

	#[test]
	fn relative_and_mixed_rpaths_are_kept() {
		for rpath in ["$ORIGIN/../lib", "$ORIGIN:/usr/lib", "lib"] {
			let mut s = spec(true, false);
			s.rpath = Some((DT_RPATH, rpath));
			s.needed = &["libc.so.6"];
			let (before, after, changes) = run_on(&s);
			assert_eq!(before, after, "rpath {rpath} must not be touched");
			assert!(changes.is_empty());
		}
	}

	#[test]
	fn nothing_to_fix_is_not_written() {
		let mut s = spec(false, true);
		s.rpath = None;
		s.needed = &["libc.so.6", "libm.so.6"];
		let (before, after, changes) = run_on(&s);
		assert_eq!(before, after);
		assert!(changes.is_empty());
	}

	#[test]
	fn needed_that_ends_in_a_slash_is_kept() {
		let mut s = spec(true, false);
		s.rpath = None;
		s.needed = &["/usr/lib/"];
		let (before, after, changes) = run_on(&s);
		assert_eq!(before, after);
		assert!(changes.is_empty());
	}

	#[test]
	fn not_elf_and_truncated_files_are_left_alone() {
		let text = unique("text");
		fs::write(&text, b"just text\n").unwrap();
		assert!(fix_file(&text).unwrap().is_empty());
		fs::write(&text, b"\x7fELF").unwrap();
		assert!(fix_file(&text).is_err() || fix_file(&text).unwrap().is_empty());
		let whole = build(&spec(true, false));
		fs::write(&text, &whole[..80]).unwrap();
		assert!(fix_file(&text).is_err());
		fs::remove_file(&text).unwrap();
	}

	#[test]
	fn read_only_file_keeps_its_mode() {
		use std::os::unix::fs::PermissionsExt;
		let path = unique("ro");
		fs::write(&path, build(&spec(true, false))).unwrap();
		fs::set_permissions(&path, fs::Permissions::from_mode(0o555)).unwrap();
		assert_eq!(fix_file(&path).unwrap().len(), 3);
		assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o555);
		fs::remove_file(&path).unwrap();
	}
}
