#undef UNICODE
#define UNICODE
#include <windows.h>
#include <stdio.h>

int main(int argc, char **argv) {
    int result;
    unsigned long bytes_written;
    LPOVERLAPPED huh = 0;
    const char *str = "Hi there, world.\n";
    HANDLE handle;

    printf("There are %d arguments:\n", argc);
    int i;
    for (i = 0; i < argc; i++) {
        printf("  [%d]: %s\n", i, argv[i]);
    }

    if (argc > 1) {
        handle = (HANDLE)strtoul(argv[1], NULL, 0);
    }
    else
    {
        handle = GetStdHandle(STD_OUTPUT_HANDLE);
    }

    result = WriteFile(handle, str, strlen(str), &bytes_written, huh);
    printf("Result: %d\n", result);

    return 0;
}