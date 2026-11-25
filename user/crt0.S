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
    mov %rdi, %r12
    mov %rsi, %r13
    /* Run C++ static constructors (.init_array) before main(). A freestanding
       C program simply has an empty list. The array bounds are addressed
       RIP-relative: an absolute `mov $symbol` emits R_X86_64_32S, which a
       dynamic (PIE) link rejects. */
    lea __init_array_start(%rip), %rbx
    lea __init_array_end(%rip), %r14
.Lconstructors:
    cmp %r14, %rbx
    jae .Lconstructors_done
    mov (%rbx), %rax
    test %rax, %rax
    jz .Lconstructors_next
    call *%rax
.Lconstructors_next:
    add $8, %rbx
    jmp .Lconstructors
.Lconstructors_done:
    mov %r12, %rdi
    mov %r13, %rsi
    /* Runtime initialization may clobber caller-saved RDX. Recompute envp
       from the original initial stack immediately before entering main(). */
    lea 16(%rsp,%rdi,8), %rdx
    call main
    mov %rax, %r12
    /* Run C++ static destructors (.fini_array) before exit(). */
    lea __fini_array_start(%rip), %rbx
    lea __fini_array_end(%rip), %r14
.Ldestructors:
    cmp %r14, %rbx
    jae .Ldestructors_done
    mov (%rbx), %rax
    test %rax, %rax
    jz .Ldestructors_next
    call *%rax
.Ldestructors_next:
    add $8, %rbx
    jmp .Ldestructors
.Ldestructors_done:
    mov %r12, %rdi
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
