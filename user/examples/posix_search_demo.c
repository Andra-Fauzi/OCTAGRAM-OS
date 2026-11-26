/*
 * Guest probe for the search/traversal/word-expansion layer of libc.
 *
 * These are the routines ported build tools reach for when they want to walk a
 * source tree, keep a symbol table, or expand a command line without a shell:
 *   - tsearch/tfind/tdelete/twalk/tdestroy (AVL backed, POSIX visits)
 *   - scandir/scandirat with alphasort/versionsort
 *   - wordexp/wordfree (quotes, $VAR, ~, globbing, error codes)
 *   - nftw/ftw directory walk with FTW_DEPTH and FTW_PHYS
 *   - basename/dirname in both the GNU and POSIX forms
 */

#include <dirent.h>
#include <errno.h>
#include <libgen.h>
#include <search.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <string.h>
#include <unistd.h>
#include <wordexp.h>
#include <fts.h>
#include <ftw.h>
#include <glob.h>

static int failures;

static void check(int condition, const char *what)
{
    if (!condition) {
        printf("posix-search: FAIL %s\n", what);
        failures++;
    }
}

/* ---- tsearch ------------------------------------------------------------ */

static const char *const search_keys[] = {"mike", "alpha", "zulu", "charlie",
                                          "bravo", "yankee", "delta"};

static int compare_keys(const void *left, const void *right)
{
    return strcmp(left, right);
}

static int walk_count;
static int walk_preorder;
static int walk_leaves;
static int walk_depth_max;

static void walk_action(const void *node, VISIT order, int depth)
{
    (void)node;
    walk_count++;
    if (order == preorder) {
        walk_preorder++;
    } else if (order == leaf) {
        walk_leaves++;
    }
    if (depth > walk_depth_max) {
        walk_depth_max = depth;
    }
}

static void test_tsearch(void)
{
    void *root = NULL;
    for (size_t index = 0; index < sizeof(search_keys) / sizeof(*search_keys);
         ++index) {
        check(tsearch(search_keys[index], &root, compare_keys) != NULL,
              "tsearch insert");
    }
    check(root != NULL, "tree root exists");
    check(tfind("charlie", &root, compare_keys) != NULL, "tfind existing key");
    check(tfind("november", &root, compare_keys) == NULL, "tfind missing key");

    walk_count = 0;
    walk_preorder = 0;
    walk_leaves = 0;
    walk_depth_max = 0;
    twalk(root, walk_action);
    /* Every node is reported once as leaf or three times as pre/post/end. */
    check(walk_count > 0, "twalk visited nodes");
    check(walk_preorder >= 1, "twalk reported preorder visits");
    check(walk_leaves >= 1, "twalk reported leaves");
    check(walk_depth_max >= 1, "twalk reported depth");

    check(tdelete("charlie", &root, compare_keys) != NULL, "tdelete existing");
    check(tfind("charlie", &root, compare_keys) == NULL, "deleted key is gone");
    check(tdelete("charlie", &root, compare_keys) == NULL,
          "tdelete missing key returns NULL");
    check(tfind("mike", &root, compare_keys) != NULL, "siblings survive delete");

    tdestroy(root, NULL);
}

/* ---- scandir ------------------------------------------------------------ */

static int only_short_names(const struct dirent *entry)
{
    return entry->d_name[0] != '.';
}

static void free_directory(struct dirent **entries, int count)
{
    for (int index = 0; index < count; ++index) {
        free(entries[index]);
    }
    free(entries);
}

static void test_scandir(void)
{
    struct dirent **entries = NULL;
    const int count = scandir("/", &entries, only_short_names, alphasort);
    check(count >= 1, "scandir returned entries");
    if (count < 1) {
        return;
    }
    check(entries[count] == NULL, "scandir array is NULL terminated");
    check(entries[0]->d_name[0] != '.', "filter removed dot entries");
    int sorted = 1;
    for (int index = 1; index < count; ++index) {
        if (strcmp(entries[index - 1]->d_name, entries[index]->d_name) > 0) {
            sorted = 0;
        }
    }
    check(sorted != 0, "alphasort sorted the names");
    check((size_t)count == (size_t)(count), "count fits");
    free_directory(entries, count);

    struct dirent **versions = NULL;
    const int version_count = scandir("/", &versions, NULL, versionsort);
    check(version_count >= 1, "versionsort scandir returned entries");
    if (version_count >= 1) {
        free_directory(versions, version_count);
    }

    check(scandir("/definitely-not-here", &entries, NULL, NULL) == -1,
          "scandir reports a missing directory");
}

/* ---- glob --------------------------------------------------------------- */

static void test_glob(void)
{
    glob_t paths;
    memset(&paths, 0, sizeof(paths));
    check(glob("/bin/sh", 0, NULL, &paths) == 0,
          "glob without GLOB_DOOFFS succeeds");
    check(paths.gl_offs == 0 && paths.gl_pathv != NULL &&
              paths.gl_pathv[0] != NULL &&
              strcmp(paths.gl_pathv[0], "/bin/sh") == 0,
          "glob without GLOB_DOOFFS has no reserved slot");
    const size_t first_count = paths.gl_pathc;
    check(glob("/bin/bash", GLOB_APPEND | GLOB_NOCHECK, NULL, &paths) == 0,
          "glob append succeeds");
    check(paths.gl_pathc == first_count + 1 &&
              strcmp(paths.gl_pathv[first_count], "/bin/bash") == 0,
          "glob append preserves earlier paths");
    globfree(&paths);

    memset(&paths, 0, sizeof(paths));
    paths.gl_offs = 2;
    check(glob("/bin/sh", GLOB_DOOFFS, NULL, &paths) == 0,
          "glob GLOB_DOOFFS succeeds");
    check(paths.gl_offs == 2 && paths.gl_pathv[0] == NULL &&
              paths.gl_pathv[1] == NULL &&
              strcmp(paths.gl_pathv[2], "/bin/sh") == 0,
          "glob preserves requested offset slots");
    globfree(&paths);
}

/* ---- wordexp ------------------------------------------------------------ */

static void test_wordexp(void)
{
    setenv("POSIX_SEARCH_VALUE", "expanded", 1);

    wordexp_t words;
    memset(&words, 0, sizeof(words));
    printf("POSIX-WORDEXP-CALL-1\n"); fflush(stdout);
    check(wordexp("alpha 'two words' \"$POSIX_SEARCH_VALUE\"", &words, 0) == 0,
          "wordexp succeeded");
    check(words.we_wordc == 3, "wordexp field count");
    if (words.we_wordc == 3) {
        check(strcmp(words.we_wordv[0], "alpha") == 0, "plain field");
        check(strcmp(words.we_wordv[1], "two words") == 0, "single quotes kept");
        check(strcmp(words.we_wordv[2], "expanded") == 0, "double quote expanded");
    }
    printf("POSIX-WORDFREE-CALL-1\n"); fflush(stdout);
    wordfree(&words);
    check(words.we_wordv == NULL && words.we_wordc == 0, "wordfree clears");

    printf("POSIX-WORDEXP-CALL-2\n"); fflush(stdout);

    check(wordexp("", &words, 0) == 0 && words.we_wordc == 0,
          "empty input yields no fields");
    printf("POSIX-WORDFREE-CALL-2\n"); fflush(stdout);
    wordfree(&words);

    printf("POSIX-WORDEXP-CALL-3\n"); fflush(stdout);

    check(wordexp("\"\"", &words, 0) == 0 && words.we_wordc == 1 &&
              words.we_wordv[0][0] == '\0',
          "quoted empty field is kept");
    printf("POSIX-WORDFREE-CALL-3\n"); fflush(stdout);
    wordfree(&words);

    printf("POSIX-WORDEXP-CALL-4\n"); fflush(stdout);

    check(wordexp("$UNSET_FOR_THE_PROBE", &words, 0) == 0 && words.we_wordc == 0,
          "undefined variable expands to nothing");
    printf("POSIX-WORDFREE-CALL-4\n"); fflush(stdout);
    wordfree(&words);

    printf("POSIX-WORDEXP-CALL-5\n"); fflush(stdout);

    check(wordexp("$UNSET_FOR_THE_PROBE", &words, WRDE_UNDEF) == WRDE_BADVAL,
          "WRDE_UNDEF rejects an undefined variable");
    printf("POSIX-WORDFREE-CALL-5\n"); fflush(stdout);
    wordfree(&words);

    printf("POSIX-WORDEXP-CALL-6\n"); fflush(stdout);

    check(wordexp("`echo hi`", &words, WRDE_NOCMD) == WRDE_CMDSUB,
          "WRDE_NOCMD reports command substitution");
    printf("POSIX-WORDFREE-CALL-6\n"); fflush(stdout);
    wordfree(&words);

    printf("POSIX-WORDEXP-CALL-7\n"); fflush(stdout);

    check(wordexp("'unterminated", &words, 0) == WRDE_SYNTAX,
          "unterminated quote is a syntax error");
    printf("POSIX-WORDFREE-CALL-7\n"); fflush(stdout);
    wordfree(&words);

    printf("POSIX-WORDEXP-CALL-8\n"); fflush(stdout);

    check(wordexp("a;b", &words, 0) == WRDE_BADCHAR, "metacharacter rejected");
    printf("POSIX-WORDFREE-CALL-8\n"); fflush(stdout);
    wordfree(&words);

    printf("POSIX-WORDEXP-CALL-9\n"); fflush(stdout);

    check(wordexp("/bin/sh*", &words, 0) == 0 && words.we_wordc >= 1,
          "glob expanded to at least the pattern itself");
    if (words.we_wordc >= 1) {
        check(strcmp(words.we_wordv[0], "/bin/sh") == 0,
              "glob matched the seeded shell");
    }
    printf("POSIX-WORDFREE-CALL-9\n"); fflush(stdout);
    wordfree(&words);

    /* WRDE_DOOFFS reserves leading slots, WRDE_APPEND adds behind them. */
    memset(&words, 0, sizeof(words));
    words.we_offs = 2;
    printf("POSIX-WORDEXP-CALL-10\n"); fflush(stdout);
    check(wordexp("one", &words, WRDE_DOOFFS) == 0, "WRDE_DOOFFS first pass");
    check(words.we_wordc == 1 && words.we_wordv[0] == NULL &&
              words.we_wordv[1] == NULL && strcmp(words.we_wordv[2], "one") == 0,
          "offset slots stay NULL");
    printf("POSIX-WORDEXP-CALL-11\n"); fflush(stdout);
    check(wordexp("two", &words, WRDE_DOOFFS | WRDE_APPEND) == 0,
          "WRDE_APPEND second pass");
    check(words.we_wordc == 2 && strcmp(words.we_wordv[2], "one") == 0 &&
              strcmp(words.we_wordv[3], "two") == 0,
          "append keeps earlier words");
    printf("POSIX-WORDFREE-CALL-10\n"); fflush(stdout);
    wordfree(&words);
}

/* ---- nftw --------------------------------------------------------------- */

static int walk_files;
static int walk_directories;
static int walk_depth_order_ok;
static int walk_post_order;

static int count_entry(const char *path, const struct stat *metadata, int type,
                       struct FTW *info)
{
    (void)path;
    (void)metadata;
    if (type == FTW_F || type == FTW_SL) {
        walk_files++;
    } else if (type == FTW_D) {
        walk_directories++;
    } else if (type == FTW_DP) {
        walk_post_order++;
    } else if (type == FTW_NS) {
        const int saved = errno;
        (void)saved;
        walk_depth_order_ok = 0;
    }
    if (info != NULL && info->level < 0) {
        walk_depth_order_ok = 0;
    }
    return 0;
}

static int count_legacy(const char *path, const struct stat *metadata, int type)
{
    (void)path;
    (void)metadata;
    if (type == FTW_F) {
        walk_files++;
    }
    return 0;
}

static void test_nftw(void)
{
    walk_files = 0;
    walk_directories = 0;
    walk_post_order = 0;
    walk_depth_order_ok = 1;
    check(nftw("/bin", count_entry, 8, FTW_PHYS) == 0, "nftw walked /bin");
    check(walk_files >= 1, "nftw reported files");
    check(walk_directories >= 1, "nftw reported the directory in preorder");

    walk_files = 0;
    walk_directories = 0;
    walk_post_order = 0;
    check(nftw("/bin", count_entry, 8, FTW_PHYS | FTW_DEPTH) == 0,
          "nftw depth-first walked /bin");
    check(walk_files >= 1, "depth walk saw files");
    check(walk_post_order >= 1, "depth walk reported FTW_DP");

    walk_files = 0;
    check(ftw("/bin", count_legacy, 8) == 0, "ftw walked /bin");
    check(walk_files >= 1, "ftw reported files");

    check(nftw("/definitely-not-here", count_entry, 8, 0) == 0,
          "nftw still calls back for a missing path");
}

/* ---- fts ---------------------------------------------------------------- */

static void test_fts(void)
{
    char *roots[] = {"/bin", NULL};
    FTS *stream = fts_open(roots, FTS_PHYSICAL, NULL);
    check(stream != NULL, "fts_open");
    if (stream == NULL) {
        return;
    }

    int directories = 0;
    int files = 0;
    int post = 0;
    FTSENT *entry = NULL;
    while ((entry = fts_read(stream)) != NULL) {
        if (entry->fts_info == FTS_D) {
            directories++;
        } else if (entry->fts_info == FTS_F) {
            files++;
        } else if (entry->fts_info == FTS_DP) {
            post++;
        }
        check(entry->fts_path != NULL && entry->fts_name[0] != '\0',
              "fts entry carries a name and a path");
    }
    check(directories >= 1, "fts reported the root directory");
    check(files >= 1, "fts reported files");
    check(post >= 1, "fts reported the post-order directory visit");
    check(fts_close(stream) == 0, "fts_close");

    /* fts_children hands the whole listing to the caller. */
    stream = fts_open(roots, FTS_PHYSICAL, NULL);
    check(stream != NULL, "fts_open for fts_children");
    if (stream == NULL) {
        return;
    }
    entry = fts_read(stream);
    check(entry != NULL && entry->fts_info == FTS_D, "first fts entry is FTS_D");
    int listed = 0;
    for (FTSENT *child = fts_children(stream, 0); child != NULL;
         child = child->fts_link) {
        listed++;
        check(child->fts_parent == entry, "child points at its parent");
    }
    check(listed >= 1, "fts_children listed entries");
    check(fts_read(stream) == entry && entry->fts_info == FTS_DP,
          "fts_read resumes with the post-order visit");
    fts_close(stream);

    /* FTS_SKIP suppresses the children of the directory just returned. */
    stream = fts_open(roots, FTS_PHYSICAL, NULL);
    check(stream != NULL, "fts_open for FTS_SKIP");
    if (stream == NULL) {
        return;
    }
    entry = fts_read(stream);
    check(entry != NULL && fts_set(stream, entry, FTS_SKIP) == 0, "fts_set SKIP");
    FTSENT *next = fts_read(stream);
    check(next == entry && entry->fts_info == FTS_DP,
          "skip yields only the post-order visit");
    check(fts_read(stream) == NULL, "skip ends the walk");
    fts_close(stream);

    /* Several roots, one after another. */
    char *many[] = {"/bin", "/dev", NULL};
    stream = fts_open(many, FTS_PHYSICAL, NULL);
    check(stream != NULL, "fts_open with two roots");
    if (stream != NULL) {
        int roots_seen = 0;
        while ((entry = fts_read(stream)) != NULL) {
            if (entry->fts_level == 0) {
                roots_seen++;
                printf("fts root: %s info=%u errno=%d\\n", entry->fts_path,
                       (unsigned)entry->fts_info, entry->fts_errno);
            }
        }
        check(roots_seen == 2, "both roots visited");
        fts_close(stream);
    }

    check(fts_open(NULL, 0, NULL) == NULL, "fts_open rejects a missing root list");
}

/* ---- basename/dirname --------------------------------------------------- */

static void test_path_parts(void)
{
    check(strcmp(basename("/usr/lib"), "lib") == 0, "GNU basename");
    check(strcmp(basename("plain"), "plain") == 0, "GNU basename without slash");

    char path_buffer[32];
    strcpy(path_buffer, "/usr/lib/");
    check(strcmp(dirname(path_buffer), "/usr") == 0, "dirname strips the leaf");

    strcpy(path_buffer, "/usr");
    check(strcmp(dirname(path_buffer), "/") == 0, "dirname of a top entry");

    strcpy(path_buffer, "relative");
    check(strcmp(dirname(path_buffer), ".") == 0, "dirname without a slash");

    strcpy(path_buffer, "/usr/lib/");
    check(strcmp(__xpg_basename(path_buffer), "lib") == 0,
          "POSIX basename strips trailing slashes");
}

int main(void)
{
    puts("POSIX-SEARCH-STAGE: tsearch");
    fflush(stdout);
    test_tsearch();
    puts("POSIX-SEARCH-STAGE: scandir");
    fflush(stdout);
    test_scandir();
    puts("POSIX-SEARCH-STAGE: glob");
    fflush(stdout);
    test_glob();
    puts("POSIX-SEARCH-STAGE: wordexp");
    fflush(stdout);
    test_wordexp();
    puts("POSIX-SEARCH-STAGE: nftw");
    fflush(stdout);
    test_nftw();
    puts("POSIX-SEARCH-STAGE: fts");
    fflush(stdout);
    test_fts();
    puts("POSIX-SEARCH-STAGE: path-parts");
    fflush(stdout);
    test_path_parts();

    if (failures != 0) {
        printf("POSIX-SEARCH-FAIL: %d check(s) failed\n", failures);
        return 1;
    }
    printf("POSIX-SEARCH-PASS\n");
    return 0;
}
