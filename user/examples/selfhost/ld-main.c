extern int selfhost_link_helper(void);

void _start(void)
{
    if (selfhost_link_helper() != 42) {
        __asm__ volatile("xor %rax, %rax\n\tmov $1, %rdi\n\tint $0x80");
    }
    __asm__ volatile("xor %rax, %rax\n\txor %rdi, %rdi\n\tint $0x80");
}
