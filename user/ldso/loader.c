#include "octagram.h"

#include <stdint.h>

#define PAGE_SIZE 4096u
#define MAX_OBJECTS 16
#define MAX_PHDRS 128
#define MAX_DYNAMIC 512
#define MAX_FILE_SIZE (16u * 1024u * 1024u)
#define MAX_NAME 256
#define USER_ADDRESS_MAX 0x0000800000000000ULL

#define PROT_NONE 0
#define PROT_READ 1
#define PROT_WRITE 2
#define PROT_EXEC 4
#define MAP_PRIVATE 2
#define MAP_FIXED 0x10
#define MAP_ANONYMOUS 0x20

#define PT_LOAD 1
#define PT_DYNAMIC 2
#define PT_PHDR 6
#define PF_X 1
#define PF_W 2
#define PF_R 4
#define ET_EXEC 2
#define ET_DYN 3
#define EM_X86_64 62
#define DT_NULL 0
#define DT_NEEDED 1
#define DT_INIT 12
#define DT_FINI 13
#define DT_HASH 4
#define DT_GNU_HASH 0x6ffffef5
#define DT_STRTAB 5
#define DT_SYMTAB 6
#define DT_RELA 7
#define DT_RELASZ 8
#define DT_RELAENT 9
#define DT_STRSZ 10
#define DT_INIT_ARRAY 25
#define DT_FINI_ARRAY 26
#define DT_INIT_ARRAYSZ 27
#define DT_FINI_ARRAYSZ 28
#define DT_SYMENT 11
#define DT_SONAME 14
#define DT_JMPREL 23
#define DT_PLTRELSZ 2
#define DT_PLTREL 20
#define DT_RELACOUNT 0x6ffffff9
#define DT_VERSYM 0x6ffffff0
#define DT_VERDEF 0x6ffffffc
#define DT_VERDEFNUM 0x6ffffffd
#define DT_VERNEED 0x6ffffffe
#define DT_VERNEEDNUM 0x6fffffff
#define R_X86_64_64 1
#define R_X86_64_PC32 2
#define R_X86_64_GLOB_DAT 6
#define R_X86_64_JUMP_SLOT 7
#define R_X86_64_COPY 5
#define R_X86_64_RELATIVE 8
#define R_X86_64_32 10
#define R_X86_64_32S 11
#define SHN_UNDEF 0
#define STB_GLOBAL 1
#define STB_WEAK 2
#define AT_NULL 0
#define AT_PHDR 3
#define AT_PHENT 4
#define AT_PHNUM 5
#define AT_ENTRY 9

typedef struct {
    unsigned char ident[16];
    uint16_t type;
    uint16_t machine;
    uint32_t version;
    uint64_t entry;
    uint64_t phoff;
    uint64_t shoff;
    uint32_t flags;
    uint16_t ehsize;
    uint16_t phentsize;
    uint16_t phnum;
    uint16_t shentsize;
    uint16_t shnum;
    uint16_t shstrndx;
} ElfHeader;

typedef struct {
    uint32_t type;
    uint32_t flags;
    uint64_t offset;
    uint64_t vaddr;
    uint64_t paddr;
    uint64_t filesz;
    uint64_t memsz;
    uint64_t align;
} ProgramHeader;

typedef struct {
    int64_t tag;
    uint64_t value;
} Dynamic;

typedef struct {
    uint32_t name;
    unsigned char info;
    unsigned char other;
    uint16_t section;
    uint64_t value;
    uint64_t size;
} Symbol;

typedef struct {
    uint64_t offset;
    uint64_t info;
    int64_t addend;
} Rela;

typedef struct {
    uint16_t version;
    uint16_t flags;
    uint16_t index;
    uint16_t count;
    uint32_t hash;
    uint32_t aux;
    uint32_t next;
} Verdef;

typedef struct {
    uint32_t name;
    uint32_t next;
} Verdaux;

typedef struct {
    uint16_t version;
    uint16_t count;
    uint32_t file;
    uint32_t aux;
    uint32_t next;
} Verneed;

typedef struct {
    uint32_t hash;
    uint16_t flags;
    uint16_t other;
    uint32_t name;
    uint32_t next;
} Vernaux;

_Static_assert(sizeof(Verdef) == 20, "Elf64_Verdef must be 20 bytes");
_Static_assert(sizeof(Verdaux) == 8, "Elf64_Verdaux must be 8 bytes");
_Static_assert(sizeof(Verneed) == 16, "Elf64_Verneed must be 16 bytes");
_Static_assert(sizeof(Vernaux) == 16, "Elf64_Vernaux must be 16 bytes");

typedef struct {
    uint64_t tag;
    uint64_t value;
} Auxiliary;

typedef struct {
    uint64_t base;
    uint64_t entry;
    ProgramHeader *headers;
    ProgramHeader header_table[MAX_PHDRS];
    uint16_t header_count;
    Dynamic *dynamic;
    uint32_t *hash;
    uint32_t *gnu_hash;
    uint64_t symbol_count;
    char *strings;
    Symbol *symbols;
    Rela *rela;
    uint64_t rela_size;
    Rela *plt_rela;
    uint64_t plt_rela_size;
    uint64_t plt_rela_type;
    uint64_t string_size;
    uint64_t symbol_size;
    char name[MAX_NAME];
    uint64_t init_address;
    uint64_t *init_array;
    uint64_t init_array_size;
    uint64_t fini_address;
    uint64_t *fini_array;
    uint64_t fini_array_size;
    uint16_t *version_table;
    Verdef *verdef;
    uint32_t verdef_count;
    Verneed *verneed;
    uint32_t verneed_count;
    int relocated;
    int loaded;
    unsigned references;
    uint64_t reservation;
    uint64_t reservation_size;
} Object;

static Object objects[MAX_OBJECTS];
static unsigned object_count;
static char library_search_path[MAX_NAME];

static void *copy_bytes(void *destination, const void *source, size_t count)
{
    unsigned char *to = destination;
    const unsigned char *from = source;
    for (size_t i = 0; i < count; ++i) {
        to[i] = from[i];
    }
    return destination;
}

static void clear_bytes(void *destination, size_t count)
{
    unsigned char *bytes = destination;
    for (size_t i = 0; i < count; ++i) {
        bytes[i] = 0;
    }
}

static size_t string_length(const char *text)
{
    size_t length = 0;
    while (text[length] != 0) {
        ++length;
    }
    return length;
}

static int string_equal(const char *left, const char *right)
{
    while (*left != 0 && *left == *right) {
        ++left;
        ++right;
    }
    return *left == *right;
}

static int string_copy(char *destination, const char *source, size_t capacity)
{
    size_t length = string_length(source);
    if (length + 1 > capacity) {
        return -1;
    }
    copy_bytes(destination, source, length + 1);
    return 0;
}

static int dynamic_string_length(const Object *object, uint64_t offset, size_t *length)
{
    if (offset >= object->string_size) {
        return -1;
    }
    uint64_t available = object->string_size - offset;
    if (available > MAX_NAME) {
        available = MAX_NAME;
    }
    const char *text = object->strings + offset;
    for (uint64_t i = 0; i < available; ++i) {
        if (text[i] == 0) {
            *length = (size_t)i;
            return 0;
        }
    }
    return -1;
}

static uint64_t align_down(uint64_t value)
{
    return value & ~((uint64_t)PAGE_SIZE - 1);
}

static uint64_t align_up(uint64_t value)
{
    return (value + PAGE_SIZE - 1) & ~((uint64_t)PAGE_SIZE - 1);
}

static void fail(const char *message) __attribute__((noreturn));

static void fail(const char *message)
{
    oct_write(OCT_STDERR, message, string_length(message));
    oct_write(OCT_STDERR, "\n", 1);
    oct_exit(127);
    __builtin_unreachable();
}

static int valid_header(const ElfHeader *header)
{
    return header->ident[0] == 0x7f && header->ident[1] == 'E'
        && header->ident[2] == 'L' && header->ident[3] == 'F'
        && header->ident[4] == 2 && header->ident[5] == 1
        && header->machine == EM_X86_64 && header->version == 1
        && (header->type == ET_EXEC || header->type == ET_DYN)
        && header->phentsize == sizeof(ProgramHeader)
        && header->phnum != 0 && header->phnum <= MAX_PHDRS;
}

static int read_file(const char *path, int *descriptor, unsigned char **contents,
                     uint64_t *length)
{
    int64_t fd = oct_open(path, OCT_O_RDONLY);
    if (fd < 0) {
        return -1;
    }
    int64_t end = oct_lseek((int)fd, 0, OCT_SEEK_END);
    if (end <= 0 || (uint64_t)end > MAX_FILE_SIZE
        || oct_lseek((int)fd, 0, OCT_SEEK_SET) < 0) {
        oct_close((int)fd);
        return -1;
    }
    void *buffer = (void *)(uintptr_t)oct_syscall6(
        16, 0, (uint64_t)end, PROT_READ | PROT_WRITE,
        MAP_PRIVATE | MAP_ANONYMOUS, (uint64_t)-1, 0);
    if ((intptr_t)buffer < 0) {
        oct_close((int)fd);
        return -1;
    }
    uint64_t done = 0;
    while (done < (uint64_t)end) {
        uint64_t amount = (uint64_t)end - done;
        if (amount > OCT_MAX_SYSCALL_IO) {
            amount = OCT_MAX_SYSCALL_IO;
        }
        int64_t result = oct_read((int)fd, (unsigned char *)buffer + done,
                                  (size_t)amount);
        if (result <= 0) {
            oct_syscall2(17, (uint64_t)(uintptr_t)buffer, (uint64_t)end);
            oct_close((int)fd);
            return -1;
        }
        done += (uint64_t)result;
    }
    *descriptor = (int)fd;
    *contents = buffer;
    *length = (uint64_t)end;
    return 0;
}

static uint64_t dynamic_pointer(const Object *object, uint64_t value)
{
    return object->base + value;
}

static uint64_t gnu_hash_symbol_count(const uint32_t *hash)
{
    uint32_t bucket_count = hash[0];
    uint32_t symbol_offset = hash[1];
    uint32_t bloom_count = hash[2];
    if (bucket_count == 0 || bucket_count > 65536
        || symbol_offset > 65536 || bloom_count == 0 || bloom_count > 65536) {
        return 0;
    }

    const uint64_t *bloom = (const uint64_t *)(hash + 4);
    const uint32_t *buckets = (const uint32_t *)(bloom + bloom_count);
    const uint32_t *chains = buckets + bucket_count;
    uint64_t count = symbol_offset;
    for (uint32_t i = 0; i < bucket_count; ++i) {
        uint32_t symbol = buckets[i];
        if (symbol < symbol_offset) {
            continue;
        }
        if (symbol - symbol_offset >= 65536) {
            return 0;
        }
        for (uint32_t scanned = 0; scanned < 65536; ++scanned, ++symbol) {
            uint32_t chain = chains[symbol - symbol_offset];
            if ((uint64_t)symbol + 1 > count) {
                count = (uint64_t)symbol + 1;
            }
            if (chain & 1) {
                break;
            }
            if (scanned == 65535) {
                return 0;
            }
        }
    }
    return count;
}

static uint64_t relocation_symbol_count(const Rela *relocations, uint64_t size)
{
    if (size % sizeof(Rela) != 0 || size / sizeof(Rela) > 131072) {
        return UINT64_MAX;
    }

    uint64_t symbol_count = 0;
    uint64_t relocation_count = size / sizeof(Rela);
    for (uint64_t i = 0; i < relocation_count; ++i) {
        uint32_t symbol_index = (uint32_t)(relocations[i].info >> 32);
        if (symbol_index >= 65536) {
            return UINT64_MAX;
        }
        if ((uint64_t)symbol_index + 1 > symbol_count) {
            symbol_count = (uint64_t)symbol_index + 1;
        }
    }
    return symbol_count;
}

static int read_dynamic(Object *object, uint64_t address)
{
    object->dynamic = (Dynamic *)(uintptr_t)address;
    for (unsigned index = 0; index < MAX_DYNAMIC; ++index) {
        Dynamic *entry = &object->dynamic[index];
        switch (entry->tag) {
        case DT_NULL:
            if (object->hash != 0) {
                object->symbol_count = object->hash[1];
            } else if (object->gnu_hash != 0) {
                object->symbol_count = gnu_hash_symbol_count(object->gnu_hash);
            }
            if ((object->rela_size != 0 && object->rela == 0)
                || (object->plt_rela_size != 0 && object->plt_rela == 0)) {
                return -1;
            }
            uint64_t rela_symbols = relocation_symbol_count(object->rela, object->rela_size);
            uint64_t plt_symbols = relocation_symbol_count(
                object->plt_rela, object->plt_rela_size);
            if (rela_symbols == UINT64_MAX || plt_symbols == UINT64_MAX) {
                return -1;
            }
            if (rela_symbols > object->symbol_count) {
                object->symbol_count = rela_symbols;
            }
            if (plt_symbols > object->symbol_count) {
                object->symbol_count = plt_symbols;
            }
            return object->strings != 0 && object->symbols != 0
                && object->symbol_count != 0
                && object->symbol_size == sizeof(Symbol) ? 0 : -1;
        case DT_INIT:
            object->init_address = dynamic_pointer(object, entry->value);
            break;
        case DT_FINI:
            object->fini_address = dynamic_pointer(object, entry->value);
            break;
        case DT_INIT_ARRAY:
            object->init_array = (uint64_t *)(uintptr_t)dynamic_pointer(object, entry->value);
            break;
        case DT_INIT_ARRAYSZ:
            if (entry->value % sizeof(uint64_t) != 0) {
                return -1;
            }
            object->init_array_size = entry->value / sizeof(uint64_t);
            break;
        case DT_FINI_ARRAY:
            object->fini_array = (uint64_t *)(uintptr_t)dynamic_pointer(object, entry->value);
            break;
        case DT_FINI_ARRAYSZ:
            if (entry->value % sizeof(uint64_t) != 0) {
                return -1;
            }
            object->fini_array_size = entry->value / sizeof(uint64_t);
            break;
        case DT_VERSYM:
            object->version_table =
                (uint16_t *)(uintptr_t)dynamic_pointer(object, entry->value);
            break;
        case DT_VERDEF:
            object->verdef =
                (Verdef *)(uintptr_t)dynamic_pointer(object, entry->value);
            break;
        case DT_VERDEFNUM:
            object->verdef_count = (uint32_t)entry->value;
            break;
        case DT_VERNEED:
            object->verneed =
                (Verneed *)(uintptr_t)dynamic_pointer(object, entry->value);
            break;
        case DT_VERNEEDNUM:
            object->verneed_count = (uint32_t)entry->value;
            break;
        case DT_HASH:
            object->hash = (uint32_t *)(uintptr_t)dynamic_pointer(object, entry->value);
            break;
        case DT_GNU_HASH:
            object->gnu_hash = (uint32_t *)(uintptr_t)dynamic_pointer(object, entry->value);
            break;
        case DT_STRTAB:
            object->strings = (char *)(uintptr_t)dynamic_pointer(object, entry->value);
            break;
        case DT_SYMTAB:
            object->symbols = (Symbol *)(uintptr_t)dynamic_pointer(object, entry->value);
            break;
        case DT_RELA:
            object->rela = (Rela *)(uintptr_t)dynamic_pointer(object, entry->value);
            break;
        case DT_RELASZ:
            object->rela_size = entry->value;
            break;
        case DT_RELAENT:
            if (entry->value != sizeof(Rela)) {
                return -1;
            }
            break;
        case DT_JMPREL:
            object->plt_rela = (Rela *)(uintptr_t)dynamic_pointer(object, entry->value);
            break;
        case DT_PLTRELSZ:
            object->plt_rela_size = entry->value;
            break;
        case DT_PLTREL:
            object->plt_rela_type = entry->value;
            break;
        case DT_STRSZ:
            object->string_size = entry->value;
            break;
        case DT_SYMENT:
            object->symbol_size = entry->value;
            break;
        default:
            break;
        }
    }
    return -1;
}

static int map_shared_object(const char *name, Object *object)
{
    char path[MAX_NAME];
    if (name[0] == '/') {
        if (string_copy(path, name, sizeof(path)) != 0) {
            return -1;
        }
    } else {
        size_t name_length = string_length(name);
        const char *cursor = library_search_path;
        if (cursor[0] == '\0') {
            cursor = "/lib";
        }
        int found = 0;
        for (;;) {
            const char *separator = cursor;
            while (*separator != '\0' && *separator != ':') {
                ++separator;
            }
            size_t directory_length = (size_t)(separator - cursor);
            if (directory_length == 0) {
                cursor = separator;
            } else {
                int slash = directory_length != 0
                    && cursor[directory_length - 1] != '/';
                if (1u + directory_length + (size_t)slash + name_length + 1 <= sizeof(path)) {
                    size_t written = 0;
                    copy_bytes(path + written, cursor, directory_length);
                    written += directory_length;
                    if (slash) {
                        path[written++] = '/';
                    }
                    copy_bytes(path + written, name, name_length + 1);
                    found = 1;
                    break;
                }
            }
            if (*separator == '\0') {
                break;
            }
            cursor = separator + 1;
        }
        if (!found) {
            const char prefix[] = "/lib/";
            size_t prefix_length = sizeof(prefix) - 1;
            if (prefix_length + name_length + 1 > sizeof(path)) {
                return -1;
            }
            copy_bytes(path, prefix, prefix_length);
            copy_bytes(path + prefix_length, name, name_length + 1);
        }
    }

    int fd;
    unsigned char *file;
    uint64_t file_length;
    if (read_file(path, &fd, &file, &file_length) != 0) {
        return -1;
    }
    if (file_length < sizeof(ElfHeader)) {
        oct_close(fd);
        return -1;
    }
    ElfHeader *header = (ElfHeader *)file;
    if (!valid_header(header) || header->type != ET_DYN
        || header->phoff > file_length
        || (uint64_t)header->phnum * sizeof(ProgramHeader) > file_length - header->phoff) {
        oct_close(fd);
        return -1;
    }

    ProgramHeader *headers = (ProgramHeader *)(file + header->phoff);
    uint64_t minimum = UINT64_MAX;
    uint64_t maximum = 0;
    for (uint16_t i = 0; i < header->phnum; ++i) {
        ProgramHeader *program = &headers[i];
        if (program->type != PT_LOAD || program->memsz == 0) {
            continue;
        }
        uint64_t memory_end = program->vaddr + program->memsz;
        if (program->filesz > program->memsz
            || program->offset > file_length
            || program->filesz > file_length - program->offset
            || memory_end < program->vaddr
            || memory_end > USER_ADDRESS_MAX - PAGE_SIZE) {
            oct_close(fd);
            return -1;
        }
        uint64_t start = align_down(program->vaddr);
        uint64_t end = align_up(program->vaddr + program->memsz);
        if (start < minimum) {
            minimum = start;
        }
        if (end > maximum) {
            maximum = end;
        }
    }
    if (minimum == UINT64_MAX || maximum <= minimum) {
        oct_close(fd);
        return -1;
    }
    uint64_t span = maximum - minimum;
    void *reservation = (void *)(uintptr_t)oct_syscall6(
        16, 0, span, PROT_NONE, MAP_PRIVATE | MAP_ANONYMOUS, (uint64_t)-1, 0);
    if ((intptr_t)reservation < 0 || (uint64_t)(uintptr_t)reservation < minimum) {
        oct_close(fd);
        return -1;
    }
    uint64_t base = (uint64_t)(uintptr_t)reservation - minimum;
    clear_bytes(object, sizeof(*object));
    object->base = base;
    object->entry = base + header->entry;
    copy_bytes(object->header_table, headers,
               (size_t)header->phnum * sizeof(ProgramHeader));
    object->headers = object->header_table;
    object->header_count = header->phnum;
    object->loaded = 1;
    object->references = 1;
    object->reservation = (uint64_t)(uintptr_t)reservation;
    object->reservation_size = span;
    if (string_copy(object->name, name, sizeof(object->name)) != 0) {
        oct_close(fd);
        return -1;
    }

    for (uint16_t i = 0; i < header->phnum; ++i) {
        ProgramHeader *program = &headers[i];
        if (program->type != PT_LOAD || program->memsz == 0) {
            continue;
        }
        uint64_t page_address = base + align_down(program->vaddr);
        uint64_t page_offset = align_down(program->offset);
        uint64_t page_delta = program->vaddr - align_down(program->vaddr);
        uint64_t map_length = align_up(page_delta + program->memsz);
        uint64_t file_page_delta = program->offset - page_offset;
        int64_t mapped;
        if (program->filesz != 0) {
            mapped = oct_syscall6(
                16, page_address, map_length, PROT_READ | PROT_WRITE,
                MAP_PRIVATE | MAP_FIXED, (uint64_t)fd, page_offset);
        } else {
            mapped = oct_syscall6(
                16, page_address, map_length, PROT_READ | PROT_WRITE,
                MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED, (uint64_t)-1, 0);
        }
        if (mapped < 0 || (uint64_t)mapped != page_address
            || file_page_delta != page_delta) {
            oct_close(fd);
            return -1;
        }
        clear_bytes((void *)(uintptr_t)(base + program->vaddr + program->filesz),
                    (size_t)(program->memsz - program->filesz));
    }

    uint64_t dynamic_address = 0;
    for (uint16_t i = 0; i < header->phnum; ++i) {
        if (headers[i].type == PT_DYNAMIC) {
            dynamic_address = base + headers[i].vaddr;
            break;
        }
    }
    oct_close(fd);
    if (dynamic_address == 0 || read_dynamic(object, dynamic_address) != 0) {
        return -1;
    }
    oct_syscall2(17, (uint64_t)(uintptr_t)file, file_length);
    return 0;
}

static uint64_t object_symbol_count(const Object *object)
{
    return object->symbol_count;
}

static const char *version_need_name(const Object *object, uint32_t index)
{
    if (object->verneed == 0 || object->strings == 0 || index < 2) {
        return 0;
    }
    uint32_t offset = 0;
    for (uint32_t entry = 0; entry < object->verneed_count && entry < 64; ++entry) {
        Verneed *need = (Verneed *)((unsigned char *)object->verneed + offset);
        uint32_t aux_offset = 0;
        for (uint16_t aux_index = 0; aux_index < need->count && aux_index < 64;
             ++aux_index) {
            Vernaux *aux = (Vernaux *)((unsigned char *)need + need->aux
                                      + aux_offset);
            if (aux->other == index) {
                return object->strings + aux->name;
            }
            if (aux->next == 0) {
                break;
            }
            aux_offset += aux->next;
        }
        if (need->next == 0) {
            break;
        }
        offset += need->next;
    }
    return 0;
}

static int version_definition(const Object *object, uint32_t symbol_index,
                              uint32_t *index_out, const char **name_out)
{
    *index_out = 0;
    if (name_out != 0) {
        *name_out = 0;
    }
    if (object->version_table == 0 || symbol_index >= object->symbol_count) {
        return 1;
    }
    uint32_t index = object->version_table[symbol_index] & 0x7fffu;
    *index_out = index;
    if (index < 2) {
        return 1;
    }
    if (object->verdef == 0 || object->strings == 0) {
        return 0;
    }
    uint32_t offset = 0;
    for (uint32_t entry = 0; entry < object->verdef_count && entry < 64; ++entry) {
        Verdef *definition = (Verdef *)((unsigned char *)object->verdef + offset);
        if (definition->index == index) {
            Verdaux *aux = (Verdaux *)((unsigned char *)definition
                                       + definition->aux);
            if (name_out != 0) {
                *name_out = object->strings + aux->name;
            }
            return 0;
        }
        if (definition->next == 0) {
            break;
        }
        offset += definition->next;
    }
    return 0;
}

/* Nama versi yang diminta referensi simbol, atau NULL untuk referensi tanpa versi. */
static const char *symbol_version_requirement(const Object *object,
                                             uint32_t symbol_index)
{
    if (object->version_table == 0 || symbol_index >= object->symbol_count) {
        return 0;
    }
    return version_need_name(object, object->version_table[symbol_index] & 0x7fffu);
}

static int symbol_version_matches(const char *required, const Object *object,
                                  uint32_t symbol_index)
{
    uint32_t index = 0;
    const char *name = 0;
    int unversioned = version_definition(object, symbol_index, &index, &name);
    if (required == 0) {
        /* Referensi tanpa versi memakai definisi tanpa versi atau versi default (2). */
        return unversioned != 0 || index == 2;
    }
    return unversioned == 0 && name != 0 && string_equal(name, required);
}

static uint64_t find_symbol_outside(const char *name, const char *required_version,
                                    const Object *excluded,
                                    Object **definition, uint64_t *size)
{
    *definition = 0;
    if (size != 0) {
        *size = 0;
    }
    for (unsigned object_index = 0; object_index < object_count; ++object_index) {
        Object *object = &objects[object_index];
        if (object == excluded) {
            continue;
        }
        uint64_t count = object_symbol_count(object);
        if (count > 65536) {
            return UINT64_MAX;
        }
        for (uint64_t index = 0; index < count; ++index) {
            Symbol *symbol = &object->symbols[index];
            unsigned binding = symbol->info >> 4;
            size_t name_length;
            if (symbol->section == SHN_UNDEF
                || dynamic_string_length(object, symbol->name, &name_length) != 0
                || (binding != STB_GLOBAL && binding != STB_WEAK)
                || !string_equal(object->strings + symbol->name, name)) {
                continue;
            }
            if (!symbol_version_matches(required_version, object, (uint32_t)index)) {
                continue;
            }
            *definition = object;
            if (size != 0) {
                *size = symbol->size;
            }
            return object->base + symbol->value;
        }
    }
    return UINT64_MAX;
}

static int resolve_rela(Object *object, Rela *relocations, uint64_t size)
{
    if (size % sizeof(Rela) != 0) {
        return -10;
    }
    uint64_t count = size / sizeof(Rela);
    if (count > 131072) {
        return -11;
    }
    for (uint64_t index = 0; index < count; ++index) {
        Rela *relocation = &relocations[index];
        unsigned type = (unsigned)relocation->info;
        uint32_t symbol_index = (uint32_t)(relocation->info >> 32);
        uint64_t *target = (uint64_t *)(uintptr_t)(object->base + relocation->offset);
        if (type == 0) {
            continue;
        }
        if (type == R_X86_64_RELATIVE) {
            *target = object->base + (uint64_t)relocation->addend;
            continue;
        }
        if (type != R_X86_64_64 && type != R_X86_64_GLOB_DAT
            && type != R_X86_64_JUMP_SLOT && type != R_X86_64_PC32
            && type != R_X86_64_32 && type != R_X86_64_32S
            && type != R_X86_64_COPY) {
            return -12;
        }
        uint64_t symbol_count = object_symbol_count(object);
        if (symbol_count > 65536 || symbol_index >= symbol_count) {
            return -13;
        }
        Symbol *symbol = &object->symbols[symbol_index];
        size_t name_length;
        if (dynamic_string_length(object, symbol->name, &name_length) != 0) {
            return -14;
        }
        const char *required_version = symbol_version_requirement(object,
                                                                  symbol_index);
        if (type == R_X86_64_COPY) {
            /* Salinan data dari library; target adalah definisi milik executable. */
            Object *definition = 0;
            uint64_t copy_size = 0;
            uint64_t source = find_symbol_outside(object->strings + symbol->name,
                                                  required_version, object,
                                                  &definition, &copy_size);
            (void)definition;
            if (source == UINT64_MAX) {
                if ((symbol->info >> 4) == STB_WEAK) {
                    continue;
                }
                return -15;
            }
            if (copy_size > 65536) {
                return -17;
            }
            if (copy_size != 0) {
                copy_bytes((void *)(uintptr_t)target,
                           (const void *)(uintptr_t)source,
                           (size_t)copy_size);
            }
            continue;
        }
        Object *definition;
        uint64_t resolved = find_symbol_outside(object->strings + symbol->name,
                                                required_version, 0,
                                                &definition, 0);
        if (resolved == UINT64_MAX) {
            if (symbol->section == SHN_UNDEF && (symbol->info >> 4) == STB_WEAK) {
                resolved = 0;
            } else {
                return -15;
            }
        }
        (void)definition;
        int64_t value = (int64_t)resolved + relocation->addend;
        if (type == R_X86_64_PC32) {
            *(int32_t *)(uintptr_t)target = (int32_t)(value - (int64_t)(uintptr_t)target);
        } else if (type == R_X86_64_32) {
            *(uint32_t *)(uintptr_t)target = (uint32_t)value;
        } else if (type == R_X86_64_32S) {
            *(int32_t *)(uintptr_t)target = (int32_t)value;
        } else {
            *target = (uint64_t)value;
        }
    }
    return 0;
}

static int relocate_object(Object *object)
{
    if (object->relocated) {
        return 0;
    }
    if (object->rela_size != 0) {
        int result = resolve_rela(object, object->rela, object->rela_size);
        if (result != 0) {
            return result;
        }
    }
    if (object->plt_rela_size != 0) {
        if (object->plt_rela_type != DT_RELA) {
            return -16;
        }
        int result = resolve_rela(object, object->plt_rela, object->plt_rela_size);
        if (result != 0) {
            return result;
        }
    }
    if (object != &objects[0]) {
        /* Executable sudah dipetakan kernel dengan permission yang benar. */
        for (uint16_t j = 0; j < object->header_count; ++j) {
            ProgramHeader *header = &object->headers[j];
            if (header->type != PT_LOAD || header->memsz == 0) {
                continue;
            }
            int protection = 0;
            if (header->flags & PF_R) protection |= PROT_READ;
            if (header->flags & PF_W) protection |= PROT_WRITE;
            if (header->flags & PF_X) protection |= PROT_EXEC;
            uint64_t start = object->base + align_down(header->vaddr);
            uint64_t length = align_up((header->vaddr - align_down(header->vaddr))
                                       + header->memsz);
            if (oct_syscall3(52, start, length, (uint64_t)protection) < 0) {
                return -2;
            }
        }
    }
    object->relocated = 1;
    return 0;
}

static int apply_relocations(void)
{
    for (unsigned i = 0; i < object_count; ++i) {
        int result = relocate_object(&objects[i]);
        if (result != 0) {
            return result;
        }
    }
    return 0;
}

static void start_application(uint64_t entry, uint64_t stack_pointer)
    __attribute__((noreturn));

static void run_object_initializers(Object *object)
{
    if (object->init_address != 0) {
        ((void (*)(void))(uintptr_t)object->init_address)();
    }
    if (object->init_array != 0) {
        for (uint64_t index = 0; index < object->init_array_size; ++index) {
            uint64_t address = object->init_array[index];
            if (address != 0) {
                ((void (*)(void))(uintptr_t)address)();
            }
        }
    }
}

static void run_initializers(void)
{
    for (unsigned i = 1; i < object_count; ++i) {
        run_object_initializers(&objects[i]);
    }
    run_object_initializers(&objects[0]);
}

static void run_object_finalizers(Object *object)
{
    if (object->fini_array != 0) {
        for (uint64_t index = object->fini_array_size; index > 0; --index) {
            uint64_t address = object->fini_array[index - 1];
            if (address != 0) {
                ((void (*)(void))(uintptr_t)address)();
            }
        }
    }
    if (object->fini_address != 0) {
        ((void (*)(void))(uintptr_t)object->fini_address)();
    }
}

static uint64_t find_symbol_in_object(const Object *object, const char *name);

/* Jalankan handler __cxa_atexit milik satu library (destructor C++) sebelum
 * petanya dilepas. DSO dikenali dari simbol __dso_handle library tersebut. */
static void run_cxa_finalizers(Object *object)
{
    Object *definition = 0;
    uint64_t finalize = find_symbol_outside("__cxa_finalize", 0, 0, &definition, 0);
    (void)definition;
    if (finalize == UINT64_MAX) {
        return;
    }
    uint64_t dso = find_symbol_in_object(object, "__dso_handle");
    if (dso == UINT64_MAX) {
        return;
    }
    ((void (*)(void *))(uintptr_t)finalize)((void *)(uintptr_t)dso);
}

static void run_finalizers(void)
{
    /* Executable lebih dulu, lalu library dalam urutan muat terbalik. */
    for (unsigned step = 0; step < object_count; ++step) {
        Object *object = step == 0 ? &objects[0] : &objects[object_count - step];
        if (object->loaded) {
            run_object_finalizers(object);
        }
    }
}

static void register_finalizer(void)
{
    Object *definition = 0;
    uint64_t address = find_symbol_outside("atexit", 0, 0, &definition, 0);
    (void)definition;
    if (address != UINT64_MAX) {
        ((int (*)(void (*)(void)))(uintptr_t)address)(run_finalizers);
    }
}

static uint64_t find_symbol_in_object(const Object *object, const char *name)
{
    uint64_t count = object_symbol_count(object);
    if (count > 65536) {
        return UINT64_MAX;
    }
    for (uint64_t index = 0; index < count; ++index) {
        Symbol *symbol = &object->symbols[index];
        unsigned binding = symbol->info >> 4;
        size_t name_length;
        if (symbol->section == SHN_UNDEF
            || dynamic_string_length(object, symbol->name, &name_length) != 0
            || (binding != STB_GLOBAL && binding != STB_WEAK)
            || !string_equal(object->strings + symbol->name, name)) {
            continue;
        }
        if (!symbol_version_matches(0, object, (uint32_t)index)) {
            continue;
        }
        return object->base + symbol->value;
    }
    return UINT64_MAX;
}

static uint64_t find_symbol_from(unsigned first, const char *name)
{
    for (unsigned index = first; index < object_count; ++index) {
        uint64_t address = find_symbol_in_object(&objects[index], name);
        if (address != UINT64_MAX) {
            return address;
        }
    }
    return UINT64_MAX;
}

/* Cari objek yang memuat alamat tertentu, dipakai oleh RTLD_NEXT. */
static int object_containing(uint64_t address)
{
    for (unsigned index = 0; index < object_count; ++index) {
        const Object *object = &objects[index];
        for (uint16_t header_index = 0; header_index < object->header_count;
             ++header_index) {
            const ProgramHeader *program = &object->headers[header_index];
            if (program->type != PT_LOAD || program->memsz == 0) {
                continue;
            }
            uint64_t start = object->base + program->vaddr;
            uint64_t end = start + program->memsz;
            if (end > start && address >= start && address < end) {
                return (int)index;
            }
        }
    }
    return -1;
}

static void *ldso_dlsym_next(const char *name, uint64_t caller)
{
    if (name == 0 || name[0] == '\0') {
        return 0;
    }
    int owner = object_containing(caller);
    unsigned first = owner < 0 ? 0u : (unsigned)owner + 1u;
    uint64_t address = find_symbol_from(first, name);
    return address == UINT64_MAX ? 0 : (void *)(uintptr_t)address;
}

static void *ldso_dlopen(const char *name)
{
    if (name == 0 || name[0] == '\0') {
        return 0;
    }
    /* Library yang sudah dimuat tidak dipetakan ulang; tambah referensi. */
    for (unsigned index = 1; index < object_count; ++index) {
        Object *existing = &objects[index];
        if (existing->loaded && string_equal(existing->name, name)) {
            ++existing->references;
            return (void *)(uintptr_t)(index + 1);
        }
    }
    Object *object = 0;
    for (unsigned index = 1; index < object_count; ++index) {
        if (!objects[index].loaded) {
            object = &objects[index];
            break;
        }
    }
    if (object == 0) {
        if (object_count >= MAX_OBJECTS) {
            return 0;
        }
        object = &objects[object_count];
        ++object_count;
    }
    if (map_shared_object(name, object) != 0) {
        if (object->reservation != 0 && object->reservation_size != 0) {
            oct_syscall2(17, object->reservation, object->reservation_size);
        }
        clear_bytes(object, sizeof(*object));
        return 0;
    }
    if (relocate_object(object) != 0) {
        if (object->reservation != 0 && object->reservation_size != 0) {
            oct_syscall2(17, object->reservation, object->reservation_size);
        }
        clear_bytes(object, sizeof(*object));
        return 0;
    }
    run_object_initializers(object);
    return (void *)(uintptr_t)((uintptr_t)(object - objects) + 1);
}

static void *ldso_dlsym(void *handle, const char *name)
{
    if (name == 0 || name[0] == '\0') {
        return 0;
    }
    if (handle == 0) {
        /* RTLD_DEFAULT: seluruh objek yang dimuat, executable lebih dulu. */
        uint64_t address = find_symbol_from(0, name);
        return address == UINT64_MAX ? 0 : (void *)(uintptr_t)address;
    }
    uintptr_t index = (uintptr_t)handle;
    if (index == 0 || index > object_count) {
        return 0;
    }
    uint64_t address = find_symbol_in_object(&objects[index - 1], name);
    return address == UINT64_MAX ? 0 : (void *)(uintptr_t)address;
}

static int ldso_dlclose(void *handle)
{
    uintptr_t index = (uintptr_t)handle;
    if (index < 2 || index > object_count) {
        return -1;
    }
    Object *object = &objects[index - 1];
    if (!object->loaded) {
        return -1;
    }
    if (object->references > 1) {
        --object->references;
        return 0;
    }
    /* Destructor harus berjalan selama kode library masih terpetakan. */
    run_object_finalizers(object);
    run_cxa_finalizers(object);
    if (object->reservation != 0 && object->reservation_size != 0) {
        if (oct_syscall2(17, object->reservation, object->reservation_size) < 0) {
            return -1;
        }
    }
    clear_bytes(object, sizeof(*object));
    return 0;
}

static void register_pointer_symbol(const char *name, void *value)
{
    Object *definition = 0;
    uint64_t address = find_symbol_outside(name, 0, 0, &definition, 0);
    (void)definition;
    if (address != UINT64_MAX) {
        *(void **)(uintptr_t)address = value;
    }
}

static void register_dynamic_loading(void)
{
    register_pointer_symbol("__oct_dlopen_hook",
                            (void *)(uintptr_t)ldso_dlopen);
    register_pointer_symbol("__oct_dlsym_hook",
                            (void *)(uintptr_t)ldso_dlsym);
    register_pointer_symbol("__oct_dlsym_next_hook",
                            (void *)(uintptr_t)ldso_dlsym_next);
    register_pointer_symbol("__oct_dlclose_hook",
                            (void *)(uintptr_t)ldso_dlclose);
}

void oct_ldso_main(uint64_t *initial_stack)
{
    uint64_t argc = initial_stack[0];
    uint64_t *environment = initial_stack + argc + 1;
    for (uint64_t index = 0; index < 64 && environment[index] != 0; ++index) {
        const char *entry = (const char *)(uintptr_t)environment[index];
        const char prefix[] = "LD_LIBRARY_PATH=";
        size_t prefix_length = sizeof(prefix) - 1;
        int matches = 1;
        for (size_t character = 0; character < prefix_length; ++character) {
            if (entry[character] != prefix[character]) {
                matches = 0;
                break;
            }
        }
        if (matches) {
            string_copy(library_search_path, entry + prefix_length,
                        sizeof(library_search_path));
            break;
        }
    }
    uint64_t *cursor = initial_stack + argc + 2;
    while (*cursor++ != 0) {
    }
    Auxiliary *auxiliary = (Auxiliary *)cursor;
    uint64_t program_headers = 0;
    uint64_t header_size = 0;
    uint64_t header_count = 0;
    uint64_t program_entry = 0;
    for (unsigned i = 0; i < 64 && auxiliary[i].tag != AT_NULL; ++i) {
        switch (auxiliary[i].tag) {
        case AT_PHDR: program_headers = auxiliary[i].value; break;
        case AT_PHENT: header_size = auxiliary[i].value; break;
        case AT_PHNUM: header_count = auxiliary[i].value; break;
        case AT_ENTRY: program_entry = auxiliary[i].value; break;
        default: break;
        }
    }
    if (program_headers == 0 || header_size != sizeof(ProgramHeader)
        || header_count == 0 || header_count > MAX_PHDRS || program_entry == 0) {
        fail("ld-octagram: invalid auxiliary vector");
    }

    clear_bytes(&objects[0], sizeof(objects[0]));
    objects[0].base = 0;
    copy_bytes(objects[0].header_table,
               (void *)(uintptr_t)program_headers,
               (size_t)header_count * sizeof(ProgramHeader));
    objects[0].headers = objects[0].header_table;
    objects[0].header_count = (uint16_t)header_count;
    uint64_t dynamic_address = 0;
    for (uint16_t i = 0; i < objects[0].header_count; ++i) {
        if (objects[0].headers[i].type == PT_PHDR) {
            if (program_headers < objects[0].headers[i].vaddr) {
                fail("ld-octagram: invalid program-header base");
            }
            objects[0].base = program_headers - objects[0].headers[i].vaddr;
        } else if (objects[0].headers[i].type == PT_DYNAMIC) {
            dynamic_address = objects[0].headers[i].vaddr;
        }
    }
    dynamic_address += objects[0].base;
    if (dynamic_address == 0 || read_dynamic(&objects[0], dynamic_address) != 0) {
        fail("ld-octagram: executable has no supported dynamic section");
    }
    object_count = 1;

    for (unsigned current = 0; current < object_count; ++current) {
        Object *object = &objects[current];
        for (unsigned index = 0; index < MAX_DYNAMIC; ++index) {
            Dynamic *entry = &object->dynamic[index];
            if (entry->tag == DT_NULL) {
                break;
            }
            if (entry->tag != DT_NEEDED) {
                continue;
            }
            size_t name_length;
            if (dynamic_string_length(object, entry->value, &name_length) != 0) {
                fail("ld-octagram: invalid dependency name");
            }
            const char *name = object->strings + entry->value;
            int already_loaded = 0;
            for (unsigned loaded = 1; loaded < object_count; ++loaded) {
                if (string_equal(objects[loaded].name, name)) {
                    already_loaded = 1;
                    break;
                }
            }
            if (already_loaded) {
                continue;
            }
            if (object_count == MAX_OBJECTS
                || map_shared_object(name, &objects[object_count]) != 0) {
                fail("ld-octagram: cannot load shared library");
            }
            ++object_count;
        }
    }

    int relocation_result = apply_relocations();
    if (relocation_result <= -10) {
        fail("ld-octagram: unsupported or unresolved relocation");
    }
    if (relocation_result == -2) {
        fail("ld-octagram: cannot set shared-library permissions");
    }
    if (relocation_result != 0) {
        fail("ld-octagram: relocation processing failed");
    }
    run_initializers();
    register_finalizer();
    register_dynamic_loading();
    start_application(program_entry, (uint64_t)(uintptr_t)initial_stack);
}

extern void ldso_transfer(uint64_t entry, uint64_t stack_pointer);

static void start_application(uint64_t entry, uint64_t stack_pointer)
{
    ldso_transfer(entry, stack_pointer);
    __builtin_unreachable();
}
