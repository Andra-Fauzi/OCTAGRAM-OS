#include <stdio.h>
#include <string.h>
#include <unistd.h>

int main(void)
{
    char formatted[16];
    int formatted_length = snprintf(formatted, sizeof(formatted), "%s:%d:%#x",
                                    "value", -12, 42u);
    if (formatted_length != 14 || strcmp(formatted, "value:-12:0x2a") != 0) {
        return 1;
    }
    char truncated[5];
    if (snprintf(truncated, sizeof(truncated), "%08x", 42u) != 8 ||
        strcmp(truncated, "0000") != 0) {
        return 2;
    }

    FILE *stream = fopen("/stdio-demo.txt", "w+");
    if (stream == NULL) {
        return 3;
    }
    if (fputs(formatted, stream) == EOF || fseek(stream, 0, SEEK_SET) != 0) {
        return 4;
    }

    char contents[sizeof(formatted)] = {0};
    size_t read_count = fread(contents, 1, sizeof(contents) - 1, stream);
    if (read_count != 14 || strcmp(contents, "value:-12:0x2a") != 0 ||
        !feof(stream)) {
        return 5;
    }
    if (fseek(stream, 0, SEEK_SET) != 0 || fgetc(stream) != 'v' ||
        ungetc('V', stream) != 'V' || ftell(stream) != 0 || fgetc(stream) != 'V') {
        return 6;
    }
    char remainder[16] = {0};
    if (fgets(remainder, sizeof(remainder), stream) != remainder ||
        strcmp(remainder, "alue:-12:0x2a") != 0 || !feof(stream)) {
        return 7;
    }
    if (fclose(stream) != 0 || unlink("/stdio-demo.txt") != 0) {
        return 8;
    }

    if (printf("stdio passed: %s (%d)\n", contents, formatted_length) < 0) {
        return 9;
    }
    return 0;
}
