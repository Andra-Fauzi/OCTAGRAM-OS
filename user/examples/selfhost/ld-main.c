extern int selfhost_link_helper(void);

void _start(void)
{
    if (selfhost_link_helper() != 42) {
        __asm__ volatile("mov $60, %rax\n\tmov $1, %rdi\n\tsyscall");
    }
    __asm__ volatile("mov $60, %rax\n\txor %rdi, %rdi\n\tsyscall");
}
