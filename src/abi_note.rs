use std::arch::global_asm;

// Embed a glibc .note.ABI-tag so that tools which that probe /proc/self/exe for
// the ABI tag identify as glibc even though sharun itself is a static musl binary
global_asm!(
	r#"
	.section .note.ABI-tag,"a",%note
	.p2align 2
	.long 4
	.long 16
	.long 1
	.asciz "GNU"
	.long 0
	.long 2
	.long 0
	.long 0
	"#
);
