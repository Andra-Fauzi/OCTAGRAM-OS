#include <stdio.h>
#include <setjmp.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

static jmp_buf jump_state;

int main(void)
{
    int signed_value = 0;
    int inferred_value = 0;
    unsigned int octal_value = 0;
    unsigned int unsigned_value = 0;
    char word[5] = {0};
    char character = 0;
    char digits[3] = {0};
    int consumed = -1;
    if (sscanf(" -42 0x2a 075 429 word Z 17!", "%d %i %o %u %4s %c %2[0-9]%n",
               &signed_value, &inferred_value, &octal_value, &unsigned_value,
               word, &character, digits, &consumed) != 7 ||
        signed_value != -42 || inferred_value != 42 || octal_value != 61 ||
        unsigned_value != 429 || strcmp(word, "word") != 0 ||
        character != 'Z' || strcmp(digits, "17") != 0 ||
        consumed <= 0) {
        return 14;
    }

    int suppressed_value = 0;
    if (sscanf("skip 123", "%*s %d", &suppressed_value) != 1 ||
        suppressed_value != 123 ||
        sscanf("not-a-number", "%d", &suppressed_value) != 0 ||
        sscanf("", "%d", &suppressed_value) != EOF) {
        return 15;
    }

    double scanned_double = 0.0;
    if (sscanf("0x1.8p+1", "%lf", &scanned_double) != 1 ||
        scanned_double != 3.0) {
        return 16;
    }

    const char directory[] = "/tcc-smoke-dir";
    struct stat metadata;
    if (mkdir(directory, 0700) != 0 ||
        stat(directory, &metadata) != 0 ||
        !S_ISDIR(metadata.st_mode) ||
        (metadata.st_mode & 0777) != 0700 ||
        rmdir(directory) != 0) {
        return 13;
    }

    char *reused = malloc(32);
    if (reused == NULL) {
        return 1;
    }
    free(reused);
    void *reused_again = malloc(16);
    if (reused_again != reused) {
        return 2;
    }
    free(reused_again);

    unsigned char *resized = malloc(4);
    if (resized == NULL) {
        return 3;
    }
    resized[0] = 0x31;
    resized[1] = 0x32;
    resized[2] = 0x33;
    resized[3] = 0x34;
    unsigned char *grown = realloc(resized, 64);
    if (grown == NULL || grown[0] != 0x31 || grown[1] != 0x32 ||
        grown[2] != 0x33 || grown[3] != 0x34) {
        return 4;
    }
    free(grown);

    unsigned char *zeroed = calloc(4, 1);
    if (zeroed == NULL || zeroed[0] != 0 || zeroed[1] != 0 ||
        zeroed[2] != 0 || zeroed[3] != 0) {
        return 5;
    }
    free(zeroed);

    unsigned char bytes[] = {0x10, 0x20, 0, 0x40};
    if (memchr(bytes, 0, sizeof(bytes)) != &bytes[2]) {
        return 6;
    }
    char combined[16] = "octa";
    if (strncat(combined, "gramOS", 3) != combined ||
        strcmp(combined, "octagra") != 0) {
        return 7;
    }
    if (strspn("123abc", "0123456789") != 3 ||
        strcspn("alpha,beta", ",") != 5) {
        return 8;
    }
    char token_text[] = "  alpha,,beta ";
    char *token_state = NULL;
    char *token = strtok_r(token_text, ", ", &token_state);
    if (token == NULL || strcmp(token, "alpha") != 0) {
        return 9;
    }
    token = strtok_r(NULL, ", ", &token_state);
    if (token == NULL || strcmp(token, "beta") != 0 ||
        strtok_r(NULL, ", ", &token_state) != NULL) {
        return 10;
    }
    char legacy_tokens[] = "red:blue";
    if (strcmp(strtok(legacy_tokens, ":"), "red") != 0 ||
        strcmp(strtok(NULL, ":"), "blue") != 0 ||
        strtok(NULL, ":") != NULL) {
        return 11;
    }

    int jump_value = setjmp(jump_state);
    if (jump_value == 0) {
        longjmp(jump_state, 24);
    }

    char *message = malloc(32);
    if (message == NULL) {
        return 12;
    }

    strcpy(message, "TCC runs on OCTAGRAM");
    printf("%s: %ld %d\n", message, strtol("24", NULL, 10), jump_value);
    free(message);
    return jump_value != 24;
}
