/*
 * Hand-written part of the dummy libsndfile.
 *
 * This stub only exists so libpulsecommon can be loaded without pulling in
 * libsndfile and its codec chain (opus, FLAC, mpg123, lame, vorbis, ogg).
 * It must only be used for applications that do not actually read or write
 * audio files through libsndfile.
 *
 * No libc is used, so the stub has no DT_NEEDED and no GLIBC_* version
 * requirement.
 */

#include <stddef.h>
#include <stdint.h>

int sf_close(void *sndfile) {
	(void) sndfile;
	return 0;
}

int sf_command(void *sndfile, int command, void *data, int datasize) {
	(void) sndfile; (void) command; (void) data; (void) datasize;
	return 0;
}

int sf_current_byterate(void *sndfile) {
	(void) sndfile;
	return 0;
}

int sf_error(void *sndfile) {
	(void) sndfile;
	return 0;
}

const char *sf_error_number(int errnum) {
	(void) errnum;
	return "libsndfile stub";
}

int sf_error_str(void *sndfile, char *str, size_t len) {
	(void) sndfile;
	if (len > 0)
		str[0] = '\0';
	return 0;
}

int sf_format_check(const void *info) {
	(void) info;
	return 0;
}

int sf_perror(void *sndfile) {
	(void) sndfile;
	return 0;
}

int64_t sf_seek(void *sndfile, int64_t frames, int whence) {
	(void) sndfile; (void) frames; (void) whence;
	return -1;
}

int sf_set_string(void *sndfile, int str_type, const char *str) {
	(void) sndfile; (void) str_type; (void) str;
	return 1;
}

const char *sf_strerror(void *sndfile) {
	(void) sndfile;
	return "libsndfile stub";
}

const char *sf_version_string(void) {
	return "libsndfile stub";
}
