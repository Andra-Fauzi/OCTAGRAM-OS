#ifndef OCTAGRAM_CTYPE_H
#define OCTAGRAM_CTYPE_H

static inline int isdigit(int character)
{
    return character >= '0' && character <= '9';
}

static inline int islower(int character)
{
    return character >= 'a' && character <= 'z';
}

static inline int isupper(int character)
{
    return character >= 'A' && character <= 'Z';
}

static inline int isalpha(int character)
{
    return islower(character) || isupper(character);
}

static inline int isalnum(int character)
{
    return isalpha(character) || isdigit(character);
}

static inline int iscntrl(int character)
{
    return (character >= 0 && character < 32) || character == 127;
}

static inline int isprint(int character)
{
    return character >= 32 && character <= 126;
}

static inline int isgraph(int character)
{
    return character > 32 && character <= 126;
}

static inline int isblank(int character)
{
    return character == ' ' || character == '\t';
}

static inline int ispunct(int character)
{
    return character >= '!' && character <= '~' && !isalnum(character);
}

static inline int isspace(int character)
{
    return character == ' ' || (character >= '\t' && character <= '\r');
}

static inline int isxdigit(int character)
{
    return isdigit(character) || (character >= 'a' && character <= 'f') ||
           (character >= 'A' && character <= 'F');
}

static inline int tolower(int character)
{
    return isupper(character) ? character - 'A' + 'a' : character;
}

static inline int toupper(int character)
{
    return islower(character) ? character - 'a' + 'A' : character;
}

#endif
