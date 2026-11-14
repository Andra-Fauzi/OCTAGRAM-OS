#include "chibicc.h"

void strarray_push(StringArray *arr, char *s) {
  if (!arr->data) {
    arr->data = calloc(8, sizeof(char *));
    if (!arr->data)
      error("out of memory");
    arr->capacity = 8;
  }

  if (arr->capacity == arr->len) {
    if (arr->capacity > INT32_MAX / 2)
      error("compiler argument list is too large");
    int capacity = arr->capacity * 2;
    char **data = calloc(capacity, sizeof(char *));
    if (!data)
      error("out of memory");
    memcpy(data, arr->data, sizeof(char *) * arr->len);
    arr->data = data;
    arr->capacity = capacity;
    for (int i = arr->len; i < arr->capacity; i++)
      arr->data[i] = NULL;
  }

  arr->data[arr->len++] = s;
}

// Takes a printf-style format string and returns a formatted string.
char *format(char *fmt, ...) {
  va_list ap;
  va_start(ap, fmt);
  va_list measure;
  va_copy(measure, ap);
  int length = vsnprintf(NULL, 0, fmt, measure);
  va_end(measure);
  if (length < 0) {
    va_end(ap);
    error("failed to format compiler output");
  }

  char *buf = calloc((size_t)length + 1, 1);
  if (!buf) {
    va_end(ap);
    error("out of memory");
  }
  int written = vsnprintf(buf, (size_t)length + 1, fmt, ap);
  va_end(ap);
  if (written != length)
    error("failed to format compiler output");
  return buf;
}
