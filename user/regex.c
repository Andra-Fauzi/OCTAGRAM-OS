/*
 * POSIX regular expressions for OCTAGRAM's C library.
 *
 * The engine is a small compiler plus a backtracking virtual machine:
 *
 *   pattern --parse--> syntax tree --emit--> instruction array --run--> match
 *
 * Instructions are a linked-thread program (CHAR, ANY, CLASS, BOL, EOL, SAVE,
 * SPLIT, JUMP, BACKREF, MATCH). SPLIT explores the first target before the
 * second, which gives greedy quantifiers; lazy quantifiers are emitted with the
 * targets swapped. Every (instruction, position) pair is visited at most once,
 * so loops over empty matches terminate and the work stays bounded.
 *
 * regexec reports the leftmost match and, among the matches that start there,
 * the longest one: every successful path updates the best end offset, so the
 * result follows the POSIX leftmost-longest rule instead of first-match-wins.
 * Subexpression positions come from the winning path. A step budget keeps a
 * pathological pattern from pinning the CPU; when it trips, the best match found
 * so far is returned (and no match if none was found).
 */

#include <ctype.h>
#include <regex.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

/* ---- Character sets ----------------------------------------------------- */

struct octagram_regex_class {
    unsigned char bits[32];
};

static void class_clear(struct octagram_regex_class *klass)
{
    memset(klass->bits, 0, sizeof(klass->bits));
}

static void class_add(struct octagram_regex_class *klass, unsigned char character)
{
    klass->bits[character >> 3] |= (unsigned char)(1u << (character & 7u));
}

static int class_has(const struct octagram_regex_class *klass,
                     unsigned char character)
{
    return (klass->bits[character >> 3] >> (character & 7u)) & 1u;
}

static void class_add_range(struct octagram_regex_class *klass, int first,
                            int last)
{
    for (int value = first; value <= last; ++value) {
        class_add(klass, (unsigned char)value);
    }
}

static void class_add_named(struct octagram_regex_class *klass, const char *name)
{
    if (strcmp(name, "alpha") == 0) {
        class_add_range(klass, 'a', 'z');
        class_add_range(klass, 'A', 'Z');
    } else if (strcmp(name, "digit") == 0) {
        class_add_range(klass, '0', '9');
    } else if (strcmp(name, "alnum") == 0) {
        class_add_range(klass, 'a', 'z');
        class_add_range(klass, 'A', 'Z');
        class_add_range(klass, '0', '9');
    } else if (strcmp(name, "lower") == 0) {
        class_add_range(klass, 'a', 'z');
    } else if (strcmp(name, "upper") == 0) {
        class_add_range(klass, 'A', 'Z');
    } else if (strcmp(name, "space") == 0) {
        class_add(klass, ' ');
        class_add(klass, '\t');
        class_add(klass, '\n');
        class_add(klass, '\r');
        class_add(klass, '\f');
        class_add(klass, '\v');
    } else if (strcmp(name, "blank") == 0) {
        class_add(klass, ' ');
        class_add(klass, '\t');
    } else if (strcmp(name, "cntrl") == 0) {
        class_add_range(klass, 0, 31);
        class_add(klass, 127);
    } else if (strcmp(name, "print") == 0) {
        class_add_range(klass, 32, 126);
    } else if (strcmp(name, "graph") == 0) {
        class_add_range(klass, 33, 126);
    } else if (strcmp(name, "punct") == 0) {
        class_add_range(klass, 33, 47);
        class_add_range(klass, 58, 64);
        class_add_range(klass, 91, 96);
        class_add_range(klass, 123, 126);
    } else if (strcmp(name, "xdigit") == 0) {
        class_add_range(klass, '0', '9');
        class_add_range(klass, 'a', 'f');
        class_add_range(klass, 'A', 'F');
    }
}

/* ---- Syntax tree -------------------------------------------------------- */

enum regex_node_kind {
    NODE_EMPTY,
    NODE_CHAR,
    NODE_ANY,
    NODE_CLASS,
    NODE_BOL,
    NODE_EOL,
    NODE_BACKREF,
    NODE_GROUP,
    NODE_CONCAT,
    NODE_ALT,
    NODE_STAR,
    NODE_PLUS,
    NODE_QUEST,
    NODE_REPEAT
};

struct octagram_regex_node {
    int kind;
    int lazy;
    int group;
    int minimum;
    int maximum;
    unsigned char character;
    struct octagram_regex_class *klass;
    struct octagram_regex_node *left;
    struct octagram_regex_node *right;
};

struct octagram_regex_parser {
    const char *pattern;
    size_t length;
    size_t position;
    int extended;
    int icase;
    int newline;
    int groups;
    int error;
};

static struct octagram_regex_node *node_new(int kind)
{
    struct octagram_regex_node *node = calloc(1, sizeof(*node));
    if (node != NULL) {
        node->kind = kind;
        node->maximum = -1;
    }
    return node;
}

/* Bracket sets move to the compiled program when the node is emitted, so the
 * tree never frees them: a repeated group emits the same set several times and
 * must not free it twice. A compile that fails after parsing can leave a few
 * sets behind; each one is 32 bytes. */
static void node_free(struct octagram_regex_node *node)
{
    if (node == NULL) {
        return;
    }
    node_free(node->left);
    node_free(node->right);
    free(node);
}

static int parser_peek(const struct octagram_regex_parser *parser, size_t ahead)
{
    const size_t offset = parser->position + ahead;
    return offset < parser->length ? (unsigned char)parser->pattern[offset] : -1;
}

/* In BRE mode the operators are the backslashed forms. */
static int parser_group_open(const struct octagram_regex_parser *parser)
{
    if (parser->extended != 0) {
        return parser_peek(parser, 0) == '(';
    }
    return parser_peek(parser, 0) == '\\' && parser_peek(parser, 1) == '(';
}

static int parser_group_close(const struct octagram_regex_parser *parser)
{
    if (parser->extended != 0) {
        return parser_peek(parser, 0) == ')';
    }
    return parser_peek(parser, 0) == '\\' && parser_peek(parser, 1) == ')';
}

static size_t parser_group_token(const struct octagram_regex_parser *parser)
{
    return parser->extended != 0 ? 1u : 2u;
}

static int parser_alternation(const struct octagram_regex_parser *parser)
{
    if (parser->extended != 0) {
        return parser_peek(parser, 0) == '|';
    }
    return parser_peek(parser, 0) == '\\' && parser_peek(parser, 1) == '|';
}

static size_t parser_alternation_token(const struct octagram_regex_parser *parser)
{
    return parser->extended != 0 ? 1u : 2u;
}

static struct octagram_regex_node *parse_expression(
    struct octagram_regex_parser *parser);

/* Parses the contents of a bracket expression. The opening '[' is consumed. */
static struct octagram_regex_class *parse_bracket(struct octagram_regex_parser *parser)
{
    struct octagram_regex_class *klass = calloc(1, sizeof(*klass));
    if (klass == NULL) {
        parser->error = REG_ESPACE;
        return NULL;
    }
    class_clear(klass);

    int negate = 0;
    if (parser_peek(parser, 0) == '^') {
        negate = 1;
        ++parser->position;
    }

    int first = 1;
    for (;;) {
        const int character = parser_peek(parser, 0);
        if (character < 0) {
            parser->error = REG_EBRACK;
            free(klass);
            return NULL;
        }
        if (character == ']' && first == 0) {
            ++parser->position;
            break;
        }
        first = 0;

        /* [:name:] */
        if (character == '[' && parser_peek(parser, 1) == ':') {
            size_t cursor = parser->position + 2;
            char name[16];
            size_t used = 0;
            while (cursor < parser->length && parser->pattern[cursor] != ':' &&
                   used + 1 < sizeof(name)) {
                name[used++] = parser->pattern[cursor++];
            }
            name[used] = '\0';
            if (cursor + 1 < parser->length && parser->pattern[cursor] == ':' &&
                parser->pattern[cursor + 1] == ']') {
                class_add_named(klass, name);
                parser->position = cursor + 2;
                continue;
            }
            parser->error = REG_ECTYPE;
            free(klass);
            return NULL;
        }

        int low = 0;
        ++parser->position;
        if (character == '\\' && parser_peek(parser, 0) >= 0) {
            const int escaped = parser_peek(parser, 0);
            ++parser->position;
            switch (escaped) {
            case 'n': low = '\n'; break;
            case 't': low = '\t'; break;
            case 'r': low = '\r'; break;
            case 'f': low = '\f'; break;
            case 'v': low = '\v'; break;
            case 'a': low = '\a'; break;
            case 'b': low = '\b'; break;
            case 'w':
                class_add_named(klass, "alnum");
                class_add(klass, '_');
                continue;
            case 'W': {
                struct octagram_regex_class word;
                class_clear(&word);
                class_add_named(&word, "alnum");
                class_add(&word, '_');
                for (int value = 0; value < 256; ++value) {
                    if (!class_has(&word, (unsigned char)value)) {
                        class_add(klass, (unsigned char)value);
                    }
                }
                continue;
            }
            case 's':
                class_add_named(klass, "space");
                continue;
            case 'S': {
                struct octagram_regex_class space;
                class_clear(&space);
                class_add_named(&space, "space");
                for (int value = 0; value < 256; ++value) {
                    if (!class_has(&space, (unsigned char)value)) {
                        class_add(klass, (unsigned char)value);
                    }
                }
                continue;
            }
            case 'd':
                class_add_named(klass, "digit");
                continue;
            case 'D': {
                for (int value = 0; value < 256; ++value) {
                    if (!isdigit(value)) {
                        class_add(klass, (unsigned char)value);
                    }
                }
                continue;
            }
            default: low = escaped; break;
            }
        } else {
            low = character;
        }

        if (parser_peek(parser, 0) == '-' && parser_peek(parser, 1) >= 0 &&
            parser_peek(parser, 1) != ']') {
            ++parser->position;
            const int high_character = parser_peek(parser, 0);
            ++parser->position;
            if (high_character < low) {
                parser->error = REG_ERANGE;
                free(klass);
                return NULL;
            }
            class_add_range(klass, low, high_character);
            continue;
        }
        class_add(klass, (unsigned char)low);
    }

    if (negate != 0) {
        for (size_t index = 0; index < sizeof(klass->bits); ++index) {
            klass->bits[index] = (unsigned char)~klass->bits[index];
        }
    }
    return klass;
}

/* Parses one atom, including a position assertion or a group. */
static struct octagram_regex_node *parse_atom(struct octagram_regex_parser *parser)
{
    if (parser->position >= parser->length) {
        return node_new(NODE_EMPTY);
    }

    if (parser_group_open(parser) != 0) {
        parser->position += parser_group_token(parser);
        if (parser->groups + 1 >= RE_NREGS) {
            parser->error = REG_ESIZE;
            return NULL;
        }
        const int group = ++parser->groups;
        struct octagram_regex_node *inner = parse_expression(parser);
        if (inner == NULL) {
            return NULL;
        }
        if (parser_group_close(parser) == 0) {
            parser->error = REG_EPAREN;
            node_free(inner);
            return NULL;
        }
        parser->position += parser_group_token(parser);
        struct octagram_regex_node *wrapper = node_new(NODE_GROUP);
        if (wrapper == NULL) {
            parser->error = REG_ESPACE;
            node_free(inner);
            return NULL;
        }
        wrapper->group = group;
        wrapper->left = inner;
        return wrapper;
    }

    const int character = parser_peek(parser, 0);

    if (character == '^') {
        ++parser->position;
        return node_new(NODE_BOL);
    }
    if (character == '$') {
        ++parser->position;
        return node_new(NODE_EOL);
    }
    if (character == '.') {
        ++parser->position;
        return node_new(NODE_ANY);
    }
    if (character == '[') {
        ++parser->position;
        struct octagram_regex_class *klass = parse_bracket(parser);
        if (klass == NULL) {
            return NULL;
        }
        struct octagram_regex_node *node = node_new(NODE_CLASS);
        if (node == NULL) {
            parser->error = REG_ESPACE;
            free(klass);
            return NULL;
        }
        node->klass = klass;
        return node;
    }
    if (character == '*') {
        /* A leading '*' is a literal in both syntaxes. */
        ++parser->position;
        struct octagram_regex_node *node = node_new(NODE_CHAR);
        if (node != NULL) {
            node->character = '*';
        }
        return node;
    }

    if (character == '\\') {
        const int escaped = parser_peek(parser, 1);
        if (escaped < 0) {
            parser->error = REG_EESCAPE;
            return NULL;
        }
        parser->position += 2;
        struct octagram_regex_node *node;
        switch (escaped) {
        case 'n':
        case 't':
        case 'r':
        case 'f':
        case 'v':
        case 'a':
        case 'b':
            node = node_new(NODE_CHAR);
            if (node != NULL) {
                node->character = (unsigned char)(escaped == 'n' ? '\n'
                                                  : escaped == 't' ? '\t'
                                                  : escaped == 'r' ? '\r'
                                                  : escaped == 'f' ? '\f'
                                                  : escaped == 'v' ? '\v'
                                                  : escaped == 'a' ? '\a'
                                                                   : '\b');
            }
            return node;
        case 'w':
        case 'W':
        case 's':
        case 'S':
        case 'd':
        case 'D': {
            struct octagram_regex_class *klass = calloc(1, sizeof(*klass));
            if (klass == NULL) {
                parser->error = REG_ESPACE;
                return NULL;
            }
            class_clear(klass);
            struct octagram_regex_class base;
            class_clear(&base);
            const int negate = (escaped == 'W' || escaped == 'S' || escaped == 'D');
            if (escaped == 'w' || escaped == 'W') {
                class_add_named(&base, "alnum");
                class_add(&base, '_');
            } else if (escaped == 's' || escaped == 'S') {
                class_add_named(&base, "space");
            } else {
                class_add_named(&base, "digit");
            }
            if (negate != 0) {
                for (int value = 0; value < 256; ++value) {
                    if (!class_has(&base, (unsigned char)value)) {
                        class_add(klass, (unsigned char)value);
                    }
                }
            } else {
                *klass = base;
            }
            node = node_new(NODE_CLASS);
            if (node == NULL) {
                free(klass);
                parser->error = REG_ESPACE;
                return NULL;
            }
            node->klass = klass;
            return node;
        }
        default:
            if (escaped >= '1' && escaped <= '9') {
                const int group = escaped - '0';
                if (group > parser->groups) {
                    parser->error = REG_ESUBREG;
                    return NULL;
                }
                node = node_new(NODE_BACKREF);
                if (node != NULL) {
                    node->group = group;
                }
                return node;
            }
            node = node_new(NODE_CHAR);
            if (node != NULL) {
                node->character = (unsigned char)escaped;
            }
            return node;
        }
    }

    ++parser->position;
    struct octagram_regex_node *node = node_new(NODE_CHAR);
    if (node != NULL) {
        node->character = (unsigned char)character;
    }
    return node;
}

/* In BRE the closing brace is the escaped form \}. */
static int at_interval_close(const struct octagram_regex_parser *parser)
{
    if (parser->extended != 0) {
        return parser_peek(parser, 0) == '}';
    }
    return parser_peek(parser, 0) == '\\' && parser_peek(parser, 1) == '}';
}

static void skip_interval_close(struct octagram_regex_parser *parser)
{
    parser->position += parser->extended != 0 ? 1u : 2u;
}

/* Parses {m}, {m,} or {m,n} after the opening brace was consumed. */
static int parse_interval(struct octagram_regex_parser *parser, int *minimum,
                          int *maximum)
{
    int low = 0;
    int digits = 0;
    while (parser_peek(parser, 0) >= '0' && parser_peek(parser, 0) <= '9') {
        if (low > 3276) {
            parser->error = REG_BADBR;
            return -1;
        }
        low = low * 10 + (parser_peek(parser, 0) - '0');
        ++parser->position;
        ++digits;
    }
    if (digits == 0) {
        parser->error = REG_BADBR;
        return -1;
    }

    int high = low;
    if (parser_peek(parser, 0) == ',') {
        ++parser->position;
        if (at_interval_close(parser) != 0) {
            high = -1;
        } else {
            high = 0;
            digits = 0;
            while (parser_peek(parser, 0) >= '0' && parser_peek(parser, 0) <= '9') {
                if (high > 3276) {
                    parser->error = REG_BADBR;
                    return -1;
                }
                high = high * 10 + (parser_peek(parser, 0) - '0');
                ++parser->position;
                ++digits;
            }
            if (digits == 0) {
                parser->error = REG_BADBR;
                return -1;
            }
        }
    }
    if (at_interval_close(parser) == 0) {
        parser->error = REG_EBRACE;
        return -1;
    }
    skip_interval_close(parser);
    if (high >= 0 && high < low) {
        parser->error = REG_BADBR;
        return -1;
    }
    *minimum = low;
    *maximum = high;
    return 0;
}

static struct octagram_regex_node *parse_piece(struct octagram_regex_parser *parser)
{
    struct octagram_regex_node *node = parse_atom(parser);
    if (node == NULL) {
        return NULL;
    }

    for (;;) {
        const int character = parser_peek(parser, 0);
        if (character == '*') {
            ++parser->position;
            struct octagram_regex_node *wrapper = node_new(NODE_STAR);
            if (wrapper == NULL) {
                parser->error = REG_ESPACE;
                node_free(node);
                return NULL;
            }
            wrapper->left = node;
            node = wrapper;
            continue;
        }
        if (parser->extended != 0 && character == '+') {
            ++parser->position;
            struct octagram_regex_node *wrapper = node_new(NODE_PLUS);
            if (wrapper == NULL) {
                parser->error = REG_ESPACE;
                node_free(node);
                return NULL;
            }
            wrapper->left = node;
            node = wrapper;
            continue;
        }
        if (parser->extended != 0 && character == '?') {
            ++parser->position;
            struct octagram_regex_node *wrapper = node_new(NODE_QUEST);
            if (wrapper == NULL) {
                parser->error = REG_ESPACE;
                node_free(node);
                return NULL;
            }
            wrapper->left = node;
            node = wrapper;
            continue;
        }
        if (parser->extended == 0 && character == '\\' && parser_peek(parser, 1) == '+') {
            parser->position += 2;
            struct octagram_regex_node *wrapper = node_new(NODE_PLUS);
            if (wrapper == NULL) {
                parser->error = REG_ESPACE;
                node_free(node);
                return NULL;
            }
            wrapper->left = node;
            node = wrapper;
            continue;
        }
        if (parser->extended == 0 && character == '\\' && parser_peek(parser, 1) == '?') {
            parser->position += 2;
            struct octagram_regex_node *wrapper = node_new(NODE_QUEST);
            if (wrapper == NULL) {
                parser->error = REG_ESPACE;
                node_free(node);
                return NULL;
            }
            wrapper->left = node;
            node = wrapper;
            continue;
        }
        if (parser->extended != 0 && character == '{') {
            const size_t saved = parser->position;
            ++parser->position;
            int minimum = 0;
            int maximum = -1;
            if (parse_interval(parser, &minimum, &maximum) == 0) {
                struct octagram_regex_node *wrapper = node_new(NODE_REPEAT);
                if (wrapper == NULL) {
                    parser->error = REG_ESPACE;
                    node_free(node);
                    return NULL;
                }
                wrapper->left = node;
                wrapper->minimum = minimum;
                wrapper->maximum = maximum;
                node = wrapper;
                continue;
            }
            if (parser->error == REG_BADBR) {
                node_free(node);
                return NULL;
            }
            if (parser->error != REG_EBRACE) {
                node_free(node);
                return NULL;
            }
            /* `{` that does not start a valid interval is a literal brace. */
            parser->error = 0;
            parser->position = saved;
            return node;
        }
        if (parser->extended == 0 && character == '\\' && parser_peek(parser, 1) == '{') {
            const size_t saved = parser->position;
            parser->position += 2;
            int minimum = 0;
            int maximum = -1;
            if (parse_interval(parser, &minimum, &maximum) == 0) {
                struct octagram_regex_node *wrapper = node_new(NODE_REPEAT);
                if (wrapper == NULL) {
                    parser->error = REG_ESPACE;
                    node_free(node);
                    return NULL;
                }
                wrapper->left = node;
                wrapper->minimum = minimum;
                wrapper->maximum = maximum;
                node = wrapper;
                continue;
            }
            parser->error = 0;
            parser->position = saved;
            return node;
        }
        break;
    }
    return node;
}

static struct octagram_regex_node *parse_sequence(struct octagram_regex_parser *parser)
{
    struct octagram_regex_node *sequence = NULL;
    for (;;) {
        const int character = parser_peek(parser, 0);
        if (character < 0) {
            break;
        }
        if (parser_alternation(parser) != 0 || parser_group_close(parser) != 0) {
            break;
        }
        struct octagram_regex_node *piece = parse_piece(parser);
        if (piece == NULL) {
            node_free(sequence);
            return NULL;
        }
        if (sequence == NULL) {
            sequence = piece;
        } else {
            struct octagram_regex_node *combined = node_new(NODE_CONCAT);
            if (combined == NULL) {
                parser->error = REG_ESPACE;
                node_free(sequence);
                node_free(piece);
                return NULL;
            }
            combined->left = sequence;
            combined->right = piece;
            sequence = combined;
        }
    }
    if (sequence == NULL) {
        sequence = node_new(NODE_EMPTY);
        if (sequence == NULL) {
            parser->error = REG_ESPACE;
        }
    }
    return sequence;
}

static struct octagram_regex_node *parse_expression(struct octagram_regex_parser *parser)
{
    struct octagram_regex_node *left = parse_sequence(parser);
    if (left == NULL) {
        return NULL;
    }
    while (parser_alternation(parser) != 0) {
        parser->position += parser_alternation_token(parser);
        struct octagram_regex_node *right = parse_sequence(parser);
        if (right == NULL) {
            node_free(left);
            return NULL;
        }
        struct octagram_regex_node *alternation = node_new(NODE_ALT);
        if (alternation == NULL) {
            parser->error = REG_ESPACE;
            node_free(left);
            node_free(right);
            return NULL;
        }
        alternation->left = left;
        alternation->right = right;
        left = alternation;
    }
    return left;
}

/* ---- Code generation ---------------------------------------------------- */

enum regex_op {
    OP_CHAR,
    OP_ANY,
    OP_CLASS,
    OP_BOL,
    OP_EOL,
    OP_SAVE,
    OP_SPLIT,
    OP_JUMP,
    OP_BACKREF,
    OP_MATCH
};

struct octagram_regex_inst {
    int op;
    int target;
    int alternate;
    int slot;
    unsigned char character;
    struct octagram_regex_class *klass;
};

struct octagram_regex_program {
    struct octagram_regex_inst *code;
    size_t size;
    size_t capacity;
    /* Bracket sets are owned by the program, not by the instruction: a
     * repeated group emits the same set several times. */
    struct octagram_regex_class **classes;
    size_t class_count;
    size_t class_capacity;
    int icase;
    int newline;
    int anchored_start;
};

struct octagram_regex_codegen {
    struct octagram_regex_program *program;
    int error;
};

static size_t emit_inst(struct octagram_regex_codegen *codegen, int op)
{
    struct octagram_regex_program *program = codegen->program;
    if (program->size == program->capacity) {
        const size_t capacity = program->capacity != 0 ? program->capacity * 2 : 32;
        struct octagram_regex_inst *grown =
            realloc(program->code, capacity * sizeof(*grown));
        if (grown == NULL) {
            codegen->error = REG_ESPACE;
            return 0;
        }
        program->code = grown;
        program->capacity = capacity;
    }
    const size_t index = program->size++;
    struct octagram_regex_inst *inst = &program->code[index];
    memset(inst, 0, sizeof(*inst));
    inst->op = op;
    return index;
}

static void emit_node(struct octagram_regex_codegen *codegen,
                      const struct octagram_regex_node *node);

static void emit_star(struct octagram_regex_codegen *codegen,
                      const struct octagram_regex_node *child, int lazy)
{
    /* split loop, exit   (greedy)   /   split exit, loop   (lazy) */
    const size_t split = emit_inst(codegen, OP_SPLIT);
    const size_t body = codegen->program->size;
    emit_node(codegen, child);
    (void)emit_inst(codegen, OP_JUMP);
    codegen->program->code[codegen->program->size - 1].target = (int)split;
    const size_t done = codegen->program->size;
    if (lazy != 0) {
        codegen->program->code[split].target = (int)done;
        codegen->program->code[split].alternate = (int)body;
    } else {
        codegen->program->code[split].target = (int)body;
        codegen->program->code[split].alternate = (int)done;
    }
}

static void emit_question(struct octagram_regex_codegen *codegen,
                          const struct octagram_regex_node *child, int lazy)
{
    const size_t split = emit_inst(codegen, OP_SPLIT);
    const size_t body = codegen->program->size;
    emit_node(codegen, child);
    const size_t done = codegen->program->size;
    if (lazy != 0) {
        codegen->program->code[split].target = (int)done;
        codegen->program->code[split].alternate = (int)body;
    } else {
        codegen->program->code[split].target = (int)body;
        codegen->program->code[split].alternate = (int)done;
    }
}

static void emit_node(struct octagram_regex_codegen *codegen,
                      const struct octagram_regex_node *node)
{
    if (node == NULL || codegen->error != 0) {
        return;
    }
    switch (node->kind) {
    case NODE_EMPTY:
        break;
    case NODE_CHAR: {
        const size_t index = emit_inst(codegen, OP_CHAR);
        codegen->program->code[index].character = node->character;
        break;
    }
    case NODE_ANY:
        (void)emit_inst(codegen, OP_ANY);
        break;
    case NODE_CLASS: {
        struct octagram_regex_program *program = codegen->program;
        int owned = 0;
        for (size_t index = 0; index < program->class_count; ++index) {
            if (program->classes[index] == node->klass) {
                owned = 1;
                break;
            }
        }
        if (owned == 0) {
            if (program->class_count == program->class_capacity) {
                const size_t capacity = program->class_capacity != 0
                                            ? program->class_capacity * 2
                                            : 8;
                struct octagram_regex_class **grown =
                    realloc(program->classes, capacity * sizeof(*grown));
                if (grown == NULL) {
                    codegen->error = REG_ESPACE;
                    return;
                }
                program->classes = grown;
                program->class_capacity = capacity;
            }
            program->classes[program->class_count++] = node->klass;
        }
        const size_t index = emit_inst(codegen, OP_CLASS);
        codegen->program->code[index].klass = node->klass;
        break;
    }
    case NODE_BOL:
        (void)emit_inst(codegen, OP_BOL);
        break;
    case NODE_EOL:
        (void)emit_inst(codegen, OP_EOL);
        break;
    case NODE_BACKREF: {
        const size_t index = emit_inst(codegen, OP_BACKREF);
        codegen->program->code[index].slot = node->group;
        break;
    }
    case NODE_GROUP: {
        const size_t start = emit_inst(codegen, OP_SAVE);
        codegen->program->code[start].slot = node->group * 2;
        emit_node(codegen, node->left);
        const size_t end = emit_inst(codegen, OP_SAVE);
        codegen->program->code[end].slot = node->group * 2 + 1;
        break;
    }
    case NODE_CONCAT:
        emit_node(codegen, node->left);
        emit_node(codegen, node->right);
        break;
    case NODE_ALT: {
        const size_t split = emit_inst(codegen, OP_SPLIT);
        const size_t left = codegen->program->size;
        emit_node(codegen, node->left);
        const size_t jump = emit_inst(codegen, OP_JUMP);
        const size_t right = codegen->program->size;
        emit_node(codegen, node->right);
        const size_t done = codegen->program->size;
        codegen->program->code[split].target = (int)left;
        codegen->program->code[split].alternate = (int)right;
        codegen->program->code[jump].target = (int)done;
        break;
    }
    case NODE_STAR:
        emit_star(codegen, node->left, node->lazy);
        break;
    case NODE_PLUS: {
        const size_t body = codegen->program->size;
        emit_node(codegen, node->left);
        const size_t split = emit_inst(codegen, OP_SPLIT);
        if (node->lazy != 0) {
            codegen->program->code[split].target = (int)codegen->program->size;
            codegen->program->code[split].alternate = (int)body;
        } else {
            codegen->program->code[split].target = (int)body;
            codegen->program->code[split].alternate = (int)codegen->program->size;
        }
        break;
    }
    case NODE_QUEST:
        emit_question(codegen, node->left, node->lazy);
        break;
    case NODE_REPEAT: {
        for (int index = 0; index < node->minimum; ++index) {
            emit_node(codegen, node->left);
        }
        if (node->maximum < 0) {
            emit_star(codegen, node->left, node->lazy);
            break;
        }
        for (int index = node->minimum; index < node->maximum; ++index) {
            emit_question(codegen, node->left, node->lazy);
        }
        break;
    }
    default:
        codegen->error = REG_BADPAT;
        break;
    }
}

/* ---- Virtual machine ---------------------------------------------------- */

#define REGEX_VISITED_CAPACITY 4096u
#define REGEX_STEP_BUDGET 4000000u

struct octagram_regex_vm {
    const struct octagram_regex_program *program;
    const char *string;
    size_t length;
    long start[RE_NREGS];
    long end[RE_NREGS];
    long best_start[RE_NREGS];
    long best_end_group[RE_NREGS];
    size_t best_end;
    int have_best;
    int flags;
    unsigned long steps;
    int aborted;
    uint64_t *visited_keys;
    uint32_t *visited_generations;
    uint32_t generation;
};

static int vm_char_equal(const struct octagram_regex_vm *vm, unsigned char left,
                         unsigned char right)
{
    if (left == right) {
        return 1;
    }
    if (vm->program->icase == 0) {
        return 0;
    }
    return tolower(left) == tolower(right);
}

static int vm_class_match(const struct octagram_regex_vm *vm,
                          const struct octagram_regex_class *klass,
                          unsigned char character)
{
    if (class_has(klass, character)) {
        return 1;
    }
    if (vm->program->icase == 0) {
        return 0;
    }
    return class_has(klass, (unsigned char)tolower(character)) ||
           class_has(klass, (unsigned char)toupper(character));
}

/* Marks (instruction, position) as visited. Returns 0 when the pair was already
 * visited, which is what keeps empty-body loops and repeated alternatives from
 * exploding. */
static int vm_visit(struct octagram_regex_vm *vm, size_t pc, size_t position)
{
    uint64_t key = ((uint64_t)pc << 32) ^ (uint64_t)position;
    if (key == UINT64_MAX) {
        key = 0;
    }
    const size_t mask = REGEX_VISITED_CAPACITY - 1;
    size_t slot = (size_t)(key * UINT64_C(0x9e3779b97f4a7c15)) & mask;
    for (;;) {
        if (vm->visited_generations[slot] != vm->generation) {
            vm->visited_generations[slot] = vm->generation;
            vm->visited_keys[slot] = key;
            return 1;
        }
        if (vm->visited_keys[slot] == key) {
            return 0;
        }
        slot = (slot + 1) & mask;
    }
}

static void vm_run(struct octagram_regex_vm *vm, size_t pc, size_t position);

static void vm_record(struct octagram_regex_vm *vm, size_t position)
{
    if (vm->have_best != 0 && position <= vm->best_end) {
        return;
    }
    vm->have_best = 1;
    vm->best_end = position;
    for (int index = 0; index < RE_NREGS; ++index) {
        vm->best_start[index] = vm->start[index];
        vm->best_end_group[index] = vm->end[index];
    }
}

static void vm_run(struct octagram_regex_vm *vm, size_t pc, size_t position)
{
    if (vm->aborted != 0) {
        return;
    }
    if (++vm->steps > REGEX_STEP_BUDGET) {
        vm->aborted = 1;
        return;
    }
    if (vm_visit(vm, pc, position) == 0) {
        return;
    }

    const struct octagram_regex_inst *inst = &vm->program->code[pc];
    switch (inst->op) {
    case OP_CHAR:
        if (position < vm->length &&
            vm_char_equal(vm, (unsigned char)vm->string[position],
                          inst->character) != 0) {
            vm_run(vm, pc + 1, position + 1);
        }
        break;
    case OP_ANY:
        if (position < vm->length) {
            if (vm->program->newline != 0 && vm->string[position] == '\n') {
                break;
            }
            vm_run(vm, pc + 1, position + 1);
        }
        break;
    case OP_CLASS:
        if (position < vm->length &&
            vm_class_match(vm, inst->klass,
                           (unsigned char)vm->string[position]) != 0) {
            vm_run(vm, pc + 1, position + 1);
        }
        break;
    case OP_BOL: {
        int at_start = position == 0;
        if (at_start == 0 && vm->program->newline != 0 &&
            vm->string[position - 1] == '\n') {
            at_start = 1;
        }
        if (at_start != 0 && (position != 0 || (vm->flags & REG_NOTBOL) == 0)) {
            vm_run(vm, pc + 1, position);
        }
        break;
    }
    case OP_EOL: {
        int at_end = position == vm->length;
        if (at_end == 0 && vm->program->newline != 0 &&
            vm->string[position] == '\n') {
            at_end = 1;
        }
        if (at_end != 0 && (position != vm->length || (vm->flags & REG_NOTEOL) == 0)) {
            vm_run(vm, pc + 1, position);
        }
        break;
    }
    case OP_SAVE: {
        const int group = inst->slot / 2;
        const int closing = inst->slot % 2;
        if (group < RE_NREGS) {
            if (closing != 0) {
                vm->end[group] = (long)position;
            } else {
                vm->start[group] = (long)position;
                vm->end[group] = -1;
            }
        }
        vm_run(vm, pc + 1, position);
        break;
    }
    case OP_BACKREF: {
        const int group = inst->slot;
        if (group < 0 || group >= RE_NREGS || vm->start[group] < 0 ||
            vm->end[group] < vm->start[group]) {
            break;
        }
        const size_t length = (size_t)(vm->end[group] - vm->start[group]);
        if (position + length > vm->length) {
            break;
        }
        int equal = 1;
        for (size_t index = 0; index < length; ++index) {
            if (vm_char_equal(vm,
                              (unsigned char)vm->string[(size_t)vm->start[group] + index],
                              (unsigned char)vm->string[position + index]) == 0) {
                equal = 0;
                break;
            }
        }
        if (equal != 0) {
            vm_run(vm, pc + 1, position + length);
        }
        break;
    }
    case OP_SPLIT:
        vm_run(vm, (size_t)inst->target, position);
        vm_run(vm, (size_t)inst->alternate, position);
        break;
    case OP_JUMP:
        vm_run(vm, (size_t)inst->target, position);
        break;
    case OP_MATCH:
        vm_record(vm, position);
        break;
    default:
        vm->aborted = 1;
        break;
    }
}

/* ---- Public API --------------------------------------------------------- */

int regcomp(regex_t *expression, const char *pattern, int flags)
{
    if (expression == NULL || pattern == NULL) {
        return REG_BADPAT;
    }
    memset(expression, 0, sizeof(*expression));

    struct octagram_regex_program *program = calloc(1, sizeof(*program));
    if (program == NULL) {
        return REG_ESPACE;
    }
    program->icase = (flags & REG_ICASE) != 0;
    program->newline = (flags & REG_NEWLINE) != 0;
    expression->re_program = program;
    expression->re_cflags = flags;

    const size_t length = strlen(pattern);
    expression->re_pattern = malloc(length + 1);
    if (expression->re_pattern == NULL) {
        regfree(expression);
        return REG_ESPACE;
    }
    memcpy(expression->re_pattern, pattern, length + 1);

    struct octagram_regex_parser parser;
    memset(&parser, 0, sizeof(parser));
    parser.pattern = pattern;
    parser.length = length;
    parser.extended = (flags & REG_EXTENDED) != 0;
    parser.icase = (flags & REG_ICASE) != 0;
    parser.newline = (flags & REG_NEWLINE) != 0;

    struct octagram_regex_node *root = parse_expression(&parser);
    if (root == NULL) {
        const int error = parser.error != 0 ? parser.error : REG_BADPAT;
        regfree(expression);
        return error;
    }
    if (parser.position != parser.length) {
        /* Unbalanced ')' or a stray group terminator. */
        node_free(root);
        regfree(expression);
        return parser_group_close(&parser) != 0 ? REG_ERPAREN : REG_BADPAT;
    }
    expression->re_nsub = (size_t)parser.groups;

    if (parser.icase != 0) {
        program->icase = 1;
    }
    program->anchored_start =
        root->kind == NODE_BOL ||
        (root->kind == NODE_CONCAT && root->left != NULL && root->left->kind == NODE_BOL);

    struct octagram_regex_codegen codegen;
    memset(&codegen, 0, sizeof(codegen));
    codegen.program = program;
    emit_node(&codegen, root);
    (void)emit_inst(&codegen, OP_MATCH);
    node_free(root);

    if (codegen.error != 0) {
        const int error = codegen.error;
        regfree(expression);
        return error;
    }
    return REG_NOERROR;
}

int regexec(const regex_t *expression, const char *string, size_t match_count,
            regmatch_t matches[], int flags)
{
    if (expression == NULL || expression->re_program == NULL || string == NULL) {
        return REG_BADPAT;
    }
    struct octagram_regex_program *program = expression->re_program;

    const size_t length = strlen(string);
    size_t range_start = 0;
    size_t range_end = length;
    if ((flags & REG_STARTEND) != 0) {
        if (matches == NULL || match_count == 0) {
            return REG_NOMATCH;
        }
        if (matches[0].rm_so > 0) {
            range_start = (size_t)matches[0].rm_so;
        }
        if (matches[0].rm_eo > 0) {
            range_end = (size_t)matches[0].rm_eo;
        }
        if (range_end > length) {
            range_end = length;
        }
        if (range_start > range_end) {
            range_start = range_end;
        }
    }
    if (range_start > length) {
        range_start = length;
    }

    uint64_t *visited_keys = calloc(REGEX_VISITED_CAPACITY, sizeof(*visited_keys));
    uint32_t *visited_generations =
        calloc(REGEX_VISITED_CAPACITY, sizeof(*visited_generations));
    if (visited_keys == NULL || visited_generations == NULL) {
        free(visited_keys);
        free(visited_generations);
        return REG_ESPACE;
    }

    struct octagram_regex_vm vm;
    memset(&vm, 0, sizeof(vm));
    vm.program = program;
    vm.string = string;
    vm.flags = flags;
    vm.visited_keys = visited_keys;
    vm.visited_generations = visited_generations;

    /* A pattern that starts with ^ can only match at the range start. */
    const size_t last_start = program->anchored_start != 0 ? range_start : range_end;
    int result = REG_NOMATCH;

    for (size_t attempt = range_start; attempt <= last_start; ++attempt) {
        vm.length = range_end;
        vm.steps = 0;
        vm.aborted = 0;
        vm.have_best = 0;
        vm.best_end = 0;
        ++vm.generation;
        for (int index = 0; index < RE_NREGS; ++index) {
            vm.start[index] = -1;
            vm.end[index] = -1;
            vm.best_start[index] = -1;
            vm.best_end_group[index] = -1;
        }
        vm.start[0] = (long)attempt;

        vm_run(&vm, 0, attempt);

        if (vm.have_best == 0) {
            if (program->anchored_start != 0) {
                break;
            }
            continue;
        }

        if ((flags & REG_NOSUB) == 0 && matches != NULL) {
            for (size_t index = 0; index < match_count; ++index) {
                matches[index].rm_so = -1;
                matches[index].rm_eo = -1;
            }
            matches[0].rm_so = (long)attempt;
            matches[0].rm_eo = (long)vm.best_end;
            const size_t groups =
                match_count - 1 < expression->re_nsub ? match_count - 1
                                                     : expression->re_nsub;
            for (size_t index = 1; index <= groups; ++index) {
                long start = vm.best_start[index];
                long end = vm.best_end_group[index];
                if (start < 0 || end < start) {
                    continue;
                }
                matches[index].rm_so = start;
                matches[index].rm_eo = end;
            }
        }
        result = REG_NOERROR;
        break;
    }

    free(visited_keys);
    free(visited_generations);
    return result;
}

size_t regerror(int error_code, const regex_t *expression, char *buffer,
                size_t length)
{
    const char *message;
    switch (error_code) {
    case REG_NOERROR: message = "Success"; break;
    case REG_NOMATCH: message = "No match"; break;
    case REG_BADPAT: message = "Invalid regular expression"; break;
    case REG_ECOLLATE: message = "Invalid collating element"; break;
    case REG_ECTYPE: message = "Invalid character class"; break;
    case REG_EESCAPE: message = "Trailing backslash"; break;
    case REG_ESUBREG: message = "Invalid back reference"; break;
    case REG_EBRACK: message = "Unmatched [ or [^"; break;
    case REG_EPAREN: message = "Unmatched ( or \\("; break;
    case REG_EBRACE: message = "Unmatched \\{"; break;
    case REG_BADBR: message = "Invalid content of \\{\\}"; break;
    case REG_ERANGE: message = "Invalid range end"; break;
    case REG_ESPACE: message = "Out of memory"; break;
    case REG_ESIZE: message = "Regular expression too big"; break;
    case REG_ERPAREN: message = "Unmatched ) or \\)"; break;
    default: message = "Unknown error"; break;
    }

    size_t needed = strlen(message);
    if (expression != NULL && expression->re_pattern != NULL) {
        needed += strlen(expression->re_pattern) + 2;
    }

    if (buffer == NULL || length == 0) {
        return needed + 1;
    }

    size_t used = 0;
    const size_t message_length = strlen(message);
    const size_t copy = message_length < length - 1 ? message_length : length - 1;
    memcpy(buffer, message, copy);
    used = copy;
    if (expression != NULL && expression->re_pattern != NULL) {
        const char *separator = ": ";
        for (const char *cursor = separator; *cursor != '\0' && used + 1 < length;
             ++cursor) {
            buffer[used++] = *cursor;
        }
        for (const char *cursor = expression->re_pattern;
             *cursor != '\0' && used + 1 < length; ++cursor) {
            buffer[used++] = *cursor;
        }
    }
    buffer[used] = '\0';
    return needed + 1;
}

void regfree(regex_t *expression)
{
    if (expression == NULL) {
        return;
    }
    struct octagram_regex_program *program = expression->re_program;
    if (program != NULL) {
        for (size_t index = 0; index < program->class_count; ++index) {
            free(program->classes[index]);
        }
        free(program->classes);
        free(program->code);
        free(program);
    }
    free(expression->re_pattern);
    expression->re_program = NULL;
    expression->re_pattern = NULL;
    expression->re_nsub = 0;
}
