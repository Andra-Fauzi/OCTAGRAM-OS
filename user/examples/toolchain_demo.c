/*
 * Guest probe for the process/table/temporary-file layer of libc.
 *
 * A build driver shells out (`popen`), keeps symbol tables (`hsearch`), makes
 * scratch files with preserved suffixes (`mkostemp`/`mkstemps`/`mkostemps`),
 * and wants real randomness for temp names (`arc4random`). It also maps files
 * through the LFS name (`mmap64`). This probe exercises all of them.
 */

#include <fcntl.h>
#include <regex.h>
#include <search.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

static int failures;

static void check(int condition, const char *what)
{
    if (!condition) {
        printf("toolchain: FAIL %s\n", what);
        failures++;
    }
}

/* ---- popen/pclose ------------------------------------------------------- */

static void test_popen_read(void)
{
    FILE *stream = popen("echo hello-from-popen", "r");
    check(stream != NULL, "popen read opened the pipeline");
    if (stream == NULL) {
        return;
    }
    char line[64];
    memset(line, 0, sizeof(line));
    check(fgets(line, sizeof(line), stream) != NULL, "popen read a line");
    check(strcmp(line, "hello-from-popen\n") == 0, "popen read the child output");
    check(pclose(stream) == 0, "pclose reported a successful reader");
}

static void test_popen_write(void)
{
    const char *path = "/tmp/popen-written.txt";
    (void)unlink(path);
    FILE *stream = popen("cat > /tmp/popen-written.txt", "w");
    check(stream != NULL, "popen write opened the pipeline");
    if (stream == NULL) {
        return;
    }
    check(fputs("written by the child\n", stream) >= 0, "popen write accepted data");
    const int status = pclose(stream);
    check(status == 0, "pclose reported a successful child");

    FILE *checker = fopen(path, "r");
    check(checker != NULL, "child created the output file");
    if (checker == NULL) {
        return;
    }
    char line[64];
    memset(line, 0, sizeof(line));
    check(fgets(line, sizeof(line), checker) != NULL, "read back child output");
    check(strcmp(line, "written by the child\n") == 0, "child output content");
    fclose(checker);
    unlink(path);

    check(popen("echo ignored", "x") == NULL, "popen rejects an invalid mode");
}

/* ---- hsearch ------------------------------------------------------------ */

static void test_hsearch(void)
{
    check(hcreate(8) != 0, "hcreate");
    ENTRY item;
    item.key = "alpha";
    item.data = (void *)1;
    check(hsearch(item, ENTER) != NULL, "hsearch ENTER inserts");
    item.data = (void *)2;
    check(hsearch(item, FIND) != NULL && hsearch(item, FIND)->data == (void *)2,
          "hsearch FIND returns the stored entry");

    ENTRY missing;
    missing.key = "omega";
    missing.data = NULL;
    check(hsearch(missing, FIND) == NULL, "hsearch FIND misses an unknown key");

    /* Enough inserts to force at least one bucket growth. */
    static char names[64][16];
    for (int index = 0; index < 64; ++index) {
        snprintf(names[index], sizeof(names[index]), "key-%d", index);
        ENTRY entry;
        entry.key = names[index];
        entry.data = (void *)(long)(index + 1);
        check(hsearch(entry, ENTER) != NULL, "hsearch growth insert");
    }
    ENTRY late;
    late.key = names[63];
    late.data = NULL;
    check(hsearch(late, FIND) != NULL && hsearch(late, FIND)->data == (void *)64,
          "entry survives table growth");
    hdestroy();

    /* Reentrant form keeps its table in the caller's struct. */
    struct hsearch_data table;
    memset(&table, 0, sizeof(table));
    check(hcreate_r(4, &table) != 0, "hcreate_r");
    ENTRY entry;
    entry.key = "beta";
    entry.data = (void *)7;
    ENTRY *found = NULL;
    check(hsearch_r(entry, ENTER, &found, &table) != 0 && found != NULL,
          "hsearch_r ENTER");
    check(hsearch_r(entry, FIND, &found, &table) != 0 && found->data == (void *)7,
          "hsearch_r FIND");
    ENTRY unknown;
    unknown.key = "gamma";
    unknown.data = NULL;
    check(hsearch_r(unknown, FIND, &found, &table) == 0, "hsearch_r miss");
    hdestroy_r(&table);
}

/* ---- temporary files ---------------------------------------------------- */

static void test_temporary_files(void)
{
    char with_suffix[] = "/tmp/octagram-XXXXXX.o";
    const int descriptor = mkstemps(with_suffix, 2);
    check(descriptor >= 0, "mkstemps created a file");
    if (descriptor >= 0) {
        check(strcmp(with_suffix + strlen(with_suffix) - 2, ".o") == 0,
              "mkstemps preserved the suffix");
        check(write(descriptor, "obj", 3) == 3, "mkstemps file is writable");
        close(descriptor);
        unlink(with_suffix);
    }

    char cloexec[] = "/tmp/octagram-cloexec-XXXXXX";
    const int cloexec_descriptor = mkostemp(cloexec, O_CLOEXEC);
    check(cloexec_descriptor >= 0, "mkostemp created a file");
    if (cloexec_descriptor >= 0) {
        const int flags = fcntl(cloexec_descriptor, F_GETFD);
        check(flags >= 0, "mkostemp descriptor is queryable");
        close(cloexec_descriptor);
        unlink(cloexec);
    }

    char both[] = "/tmp/octagram-both-XXXXXX.txt";
    const int both_descriptor = mkostemps(both, 4, O_CLOEXEC);
    check(both_descriptor >= 0, "mkostemps created a file");
    if (both_descriptor >= 0) {
        check(strcmp(both + strlen(both) - 4, ".txt") == 0,
              "mkostemps preserved the suffix");
        close(both_descriptor);
        unlink(both);
    }

    check(mkstemps(with_suffix, -1) == -1, "mkstemps rejects a negative suffix");
}

/* ---- arc4random --------------------------------------------------------- */

static void test_arc4random(void)
{
    unsigned char first[32];
    unsigned char second[32];
    memset(first, 0, sizeof(first));
    memset(second, 0, sizeof(second));
    arc4random_buf(first, sizeof(first));
    arc4random_buf(second, sizeof(second));
    check(memcmp(first, second, sizeof(first)) != 0,
          "two buffers differ");

    int distinct = 0;
    for (int index = 0; index < 32; ++index) {
        if (first[index] != 0) {
            distinct = 1;
        }
    }
    check(distinct != 0, "buffer is not all zeroes");

    check(arc4random_uniform(1) == 0, "uniform(1) is zero");
    int out_of_range = 0;
    for (int index = 0; index < 256; ++index) {
        const uint32_t value = arc4random_uniform(7);
        if (value >= 7) {
            out_of_range = 1;
        }
    }
    check(out_of_range == 0, "uniform bound respected");
}

/* ---- mmap64 ------------------------------------------------------------- */

static void test_mmap64(void)
{
    const size_t length = 4096;
    unsigned char *mapping =
        mmap64(NULL, length, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS,
               -1, 0);
    check(mapping != MAP_FAILED, "mmap64 mapped an anonymous page");
    if (mapping == MAP_FAILED) {
        return;
    }
    mapping[0] = 0x5a;
    mapping[length - 1] = 0x2b;
    check(mapping[0] == 0x5a && mapping[length - 1] == 0x2b,
          "mmap64 mapping is readable and writable");
    check(munmap(mapping, length) == 0, "munmap released the mapping");
}

/* ---- regex -------------------------------------------------------------- */

static void check_regex(const char *pattern, int flags, const char *text,
                        long expected_start, long expected_end, const char *what)
{
    regex_t expression;
    check(regcomp(&expression, pattern, flags) == 0, what);
    regmatch_t matches[4];
    const int status = regexec(&expression, text, 4, matches, 0);
    if (expected_start < 0) {
        check(status == REG_NOMATCH, what);
    } else {
        check(status == 0 && matches[0].rm_so == expected_start &&
                  matches[0].rm_eo == expected_end,
              what);
    }
    regfree(&expression);
}

static void test_regex(void)
{
    check_regex("abc", REG_EXTENDED, "xabcx", 1, 4, "ERE literal");
    check_regex("a+b", REG_EXTENDED, "aaab", 0, 4, "ERE plus");
    check_regex(".*", REG_EXTENDED, "abc", 0, 3, "ERE dot star");
    check_regex("a|ab", REG_EXTENDED, "ab", 0, 2, "ERE leftmost-longest");
    check_regex("^foo$", REG_EXTENDED, "foo", 0, 3, "ERE anchors");
    check_regex("[[:digit:]]+", REG_EXTENDED, "ab123cd", 2, 5, "ERE class");
    check_regex("[^0-9]+", REG_EXTENDED, "12ab34", 2, 4, "ERE negated class");
    check_regex("a{2,3}", REG_EXTENDED, "aaaa", 0, 3, "ERE interval");
    check_regex("hello", REG_ICASE, "HeLLo", 0, 5, "REG_ICASE");
    check_regex("[abc", REG_EXTENDED, "abc", -1, 0, "unmatched bracket");
    check_regex("\\(ab\\)*c", 0, "ababc", 0, 5, "BRE group star");
    check_regex("\\(a\\)\\1", 0, "xaay", 1, 3, "BRE back reference");
    check_regex("a\\{2\\}", 0, "xaay", 1, 3, "BRE interval");

    regex_t expression;
    check(regcomp(&expression, "(a)(b)?", REG_EXTENDED) == 0, "group compile");
    regmatch_t matches[3];
    check(regexec(&expression, "a", 3, matches, 0) == 0, "optional group exec");
    check(matches[1].rm_so == 0 && matches[1].rm_eo == 1, "group 1 span");
    check(matches[2].rm_so == -1 && matches[2].rm_eo == -1,
          "unmatched group reports -1");
    check(expression.re_nsub == 2, "re_nsub counts the groups");
    regfree(&expression);

    char message[64];
    check(regerror(REG_BADPAT, NULL, message, sizeof(message)) > 0,
          "regerror returns a length");
    check(strstr(message, "regular expression") != NULL, "regerror text");

    check(regcomp(&expression, "(a*)*b", REG_EXTENDED) == 0, "empty-body loop");
    check(regexec(&expression, "aaab", 1, matches, 0) == 0 &&
              matches[0].rm_eo == 4,
          "empty-body loop terminates");
    regfree(&expression);

    check(regcomp(&expression, "a[", REG_EXTENDED) == REG_EBRACK,
          "regcomp reports REG_EBRACK");
    check(regcomp(&expression, "(ab", REG_EXTENDED) == REG_EPAREN,
          "regcomp reports REG_EPAREN");
    check(regcomp(&expression, "a{3,1}", REG_EXTENDED) == REG_BADBR,
          "regcomp reports REG_BADBR");
    check(regcomp(&expression, "a\\", REG_EXTENDED) == REG_EESCAPE,
          "regcomp reports REG_EESCAPE");
}

/* ---- strptime ----------------------------------------------------------- */

static void test_strptime(void)
{
    struct tm value;
    memset(&value, 0, sizeof(value));
    const char *rest = strptime("2026-11-25 13:45:07 tail", "%Y-%m-%d %H:%M:%S",
                                &value);
    check(rest != NULL, "strptime parsed a timestamp");
    if (rest != NULL) {
        check(strcmp(rest, " tail") == 0, "strptime stopped after the timestamp");
        check(value.tm_year == 126, "strptime stored the year offset");
        check(value.tm_mon == 10, "strptime stored the month");
        check(value.tm_mday == 25, "strptime stored the day");
        check(value.tm_hour == 13 && value.tm_min == 45 && value.tm_sec == 7,
              "strptime stored the time of day");
    }

    memset(&value, 0, sizeof(value));
    rest = strptime("Wed, 25 Nov 2026 23:30:00 GMT", "%a, %d %b %Y %H:%M:%S %Z",
                    &value);
    check(rest != NULL && *rest == '\0', "strptime parsed an HTTP date");
    if (rest != NULL) {
        check(value.tm_mon == 10 && value.tm_mday == 25 && value.tm_hour == 23,
              "HTTP date fields");
        check(value.tm_wday == 3, "strptime stored the weekday name");
    }

    memset(&value, 0, sizeof(value));
    rest = strptime("11:05 pm", "%I:%M %p", &value);
    check(rest != NULL && value.tm_hour == 23 && value.tm_min == 5,
          "12-hour clock with pm");

    memset(&value, 0, sizeof(value));
    rest = strptime("12:30 am", "%I:%M %p", &value);
    check(rest != NULL && value.tm_hour == 0, "12-hour clock midnight wrap");

    memset(&value, 0, sizeof(value));
    check(strptime("not-a-date", "%Y-%m-%d", &value) == NULL,
          "strptime rejects mismatched input");
    check(strptime("2026-11-25", "%Y-%m-%d %H:%M", &value) == NULL,
          "strptime rejects truncated input");

    /* Round trip through strftime. */
    char rendered[32];
    memset(&value, 0, sizeof(value));
    value.tm_year = 126;
    value.tm_mon = 10;
    value.tm_mday = 25;
    const size_t written = strftime(rendered, sizeof(rendered), "%Y-%m-%d", &value);
    check(written == 10, "strftime rendered ten characters");
    struct tm reparsed;
    memset(&reparsed, 0, sizeof(reparsed));
    check(strptime(rendered, "%Y-%m-%d", &reparsed) != NULL,
          "strptime read the rendered date back");
    check(reparsed.tm_year == value.tm_year && reparsed.tm_mon == value.tm_mon &&
              reparsed.tm_mday == value.tm_mday,
          "round trip preserved the date");
}

int main(void)
{
    test_popen_read();
    test_popen_write();
    test_hsearch();
    test_temporary_files();
    test_arc4random();
    test_mmap64();
    test_regex();
    test_strptime();

    if (failures != 0) {
        printf("TOOLCHAIN-FAIL: %d check(s) failed\n", failures);
        return 1;
    }
    printf("TOOLCHAIN-PASS\n");
    return 0;
}
