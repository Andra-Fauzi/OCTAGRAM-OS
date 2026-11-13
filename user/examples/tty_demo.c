#include <octagram.h>

int main(void)
{
    char input[128];
    static const char prompt[] = "Type a line and press Enter: ";
    static const char result[] = "You typed: ";

    oct_write(OCT_STDOUT, prompt, sizeof(prompt) - 1);
    int64_t count = oct_read(OCT_STDIN, input, sizeof(input));
    if (count < 0) {
        static const char error[] = "keyboard input failed\n";
        oct_write(OCT_STDERR, error, sizeof(error) - 1);
        return 1;
    }

    oct_write(OCT_STDOUT, result, sizeof(result) - 1);
    oct_write(OCT_STDOUT, input, (size_t)count);
    return 0;
}
