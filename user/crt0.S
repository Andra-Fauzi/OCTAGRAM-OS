.section .text.start,"ax",@progbits
.global _start
.type _start,@function
.weak __oct_tls_initialize
.weak __errno_location
.weak __h_errno_location
.weak exit
_start:
    xor %rbp, %rbp
    mov (%rsp), %rdi
    lea 8(%rsp), %rsi
    lea 16(%rsp,%rdi,8), %rdx
    mov environ@GOTPCREL(%rip), %rax
    mov %rdx, (%rax)
    push %rdi
    push %rsi
    and $-16, %rsp
    mov __oct_tls_initialize@GOTPCREL(%rip), %rax
    test %rax, %rax
    jz .Ltls_ready
    call *%rax
    test %eax, %eax
    jnz .Lstartup_failure
.Ltls_ready:
    /* Initialize shared-libc per-thread error state before main(). */
    mov __errno_location@GOTPCREL(%rip), %rax
    test %rax, %rax
    jz .Lerrno_ready
    call *%rax
.Lerrno_ready:
    mov __h_errno_location@GOTPCREL(%rip), %rax
    test %rax, %rax
    jz .Lh_errno_ready
    call *%rax
.Lh_errno_ready:
    pop %rsi
    pop %rdi
    call main
    mov %rax, %rdi
    mov exit@GOTPCREL(%rip), %rax
    test %rax, %rax
    jz .Ldirect_exit
    call *%rax
.Ldirect_exit:
    xor %rax, %rax
    int $0x80
2:
    pause
    jmp 2b
.Lstartup_failure:
    mov $127, %rdi
    xor %rax, %rax
    int $0x80
1:
    pause
    jmp 1b
.size _start, .-_start

.section .note.GNU-stack,"",@progbits
