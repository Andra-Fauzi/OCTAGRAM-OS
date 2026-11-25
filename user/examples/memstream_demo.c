/*
 * Guest probe for memory-backed stdio streams and descriptor formatting.
 *
 * Covers the pieces an LLVM- or rustc-based toolchain relies on when it wants
 * to build a diagnostic string without touching the filesystem:
 *   - open_memstream(): growing buffer, published through caller pointers on
 *     flush and close, always NUL terminated, size excludes the terminator;
 *   - fmemopen(): fixed caller buffer, read/write/append modes, short writes at
 *     capacity, zero-filled holes when seeking past the end;
 *   - dprintf(): formatted output straight to a descriptor.
 */

#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

static int failures;

static void check(int condition, const char *what)
{
    if (!condition) {
        printf("memstream: FAIL %s\n", what);
        failures++;
    }
}

static int test_open_memstream(void)
{
    char *buffer = NULL;
    size_t size = 0;
    FILE *stream = open_memstream(&buffer, &size);
    check(stream != NULL, "open_memstream returned a stream");
    if (stream == NULL) {
        return -1;
    }
    check(buffer == NULL && size == 0, "open_memstream starts empty");

    fprintf(stream, "alpha %d ", 7);
    fputs("beta", stream);
    check(fflush(stream) == 0, "open_memstream flush");
    check(size == strlen("alpha 7 beta"), "size after flush");
    check(buffer != NULL && strcmp(buffer, "alpha 7 beta") == 0,
          "content after flush");

    /* Writing after a flush must keep growing the same published buffer. */
    fputc('\n', stream);
    check(fclose(stream) == 0, "open_memstream close");
    check(size == strlen("alpha 7 beta\n"), "size after close");
    check(buffer != NULL && strcmp(buffer, "alpha 7 beta\n") == 0,
          "content after close");
    check(buffer != NULL && buffer[size] == '\0', "NUL terminator after close");

    int descriptor = fileno(stream);
    check(descriptor < 0, "closed memory stream has no descriptor");

    /* Seeking forward leaves a zero-filled hole behind, like a sparse file. */
    size = 0;
    buffer = NULL;
    stream = open_memstream(&buffer, &size);
    check(stream != NULL, "second open_memstream");
    if (stream == NULL) {
        return -1;
    }
    fputs("ab", stream);
    check(fseek(stream, 4, SEEK_SET) == 0, "seek past end");
    fputs("cd", stream);
    check(fclose(stream) == 0, "second close");
    check(size == 6, "hole size");
    check(buffer != NULL && buffer[0] == 'a' && buffer[1] == 'b' &&
              buffer[2] == '\0' && buffer[3] == '\0' && buffer[4] == 'c' &&
              buffer[5] == 'd',
          "hole is zero-filled");
    return 0;
}

static int test_fmemopen_read(void)
{
    char text[16];
    memcpy(text, "read me", 8);

    FILE *reader = fmemopen(text, 7, "r");
    check(reader != NULL, "fmemopen read");
    if (reader == NULL) {
        return -1;
    }
    char line[8];
    memset(line, 0, sizeof(line));
    check(fread(line, 1, sizeof(line), reader) == 7, "read whole buffer");
    check(memcmp(line, "read me", 7) == 0, "read content");
    check(fgetc(reader) == EOF, "read hits EOF at buffer end");
    check(feof(reader) != 0, "EOF flag set");
    check(fileno(reader) < 0, "fmemopen has no descriptor");
    check(fclose(reader) == 0, "fmemopen read close");
    return 0;
}

static int test_fmemopen_write(void)
{
    char scratch[8];
    memset(scratch, '#', sizeof(scratch));

    FILE *writer = fmemopen(scratch, sizeof(scratch), "w");
    check(writer != NULL, "fmemopen write");
    if (writer == NULL) {
        return -1;
    }
    check(fwrite("abc", 1, 3, writer) == 3, "write three bytes");
    check(fseek(writer, 1, SEEK_SET) == 0, "rewind to offset 1");
    check(fputc('Z', writer) == 'Z', "overwrite in place");
    check(ftell(writer) == 2, "tell reports offset");
    check(memcmp(scratch, "aZc", 3) == 0, "overwrite visible in buffer");

    /* Only the bytes that still fit are accepted; the rest is a short write. */
    check(fwrite("0123456789", 1, 10, writer) == 6, "short write at capacity");
    check(ftell(writer) == 8, "offset stops at capacity");
    check(fseek(writer, 0, SEEK_END) == 0, "seek to end");
    check(fputc('!', writer) == EOF, "write past capacity fails");
    check(fclose(writer) == 0, "fmemopen write close");
    return 0;
}

static int test_fmemopen_append(void)
{
    char text[16];
    memset(text, 0, sizeof(text));
    memcpy(text, "head", 4);

    FILE *appender = fmemopen(text, sizeof(text), "a+");
    check(appender != NULL, "fmemopen append");
    if (appender == NULL) {
        return -1;
    }
    check(ftell(appender) == 4, "append starts at first NUL");
    check(fputs("-tail", appender) == 0, "append write");
    check(fclose(appender) == 0, "append close");
    check(strcmp(text, "head-tail") == 0, "append keeps existing bytes");
    return 0;
}

static int test_dprintf(void)
{
    const char *path = "/memstream-dprintf.txt";
    int descriptor = open(path, O_WRONLY | O_CREAT | O_TRUNC, 0644);
    check(descriptor >= 0, "open dprintf target");
    if (descriptor < 0) {
        return -1;
    }
    const char *expected = "dprintf 42 !\n";
    const int written = dprintf(descriptor, "%s %d %c\n", "dprintf", 42, '!');
    check(written == (int)strlen(expected), "dprintf byte count");
    check(close(descriptor) == 0, "close dprintf target");

    FILE *checker = fopen(path, "r");
    check(checker != NULL, "reopen dprintf target");
    if (checker == NULL) {
        return -1;
    }
    char stored[32];
    memset(stored, 0, sizeof(stored));
    check(fgets(stored, sizeof(stored), checker) != NULL, "read back dprintf");
    check(strcmp(stored, expected) == 0, "dprintf content");
    fclose(checker);

    check(dprintf(-1, "ignored\n") == -1, "dprintf rejects a bad descriptor");
    return 0;
}

int main(void)
{
    test_open_memstream();
    test_fmemopen_read();
    test_fmemopen_write();
    test_fmemopen_append();
    test_dprintf();

    if (failures != 0) {
        printf("MEMSTREAM-FAIL: %d check(s) failed\n", failures);
        return 1;
    }
    printf("MEMSTREAM-PASS\n");
    return 0;
}
