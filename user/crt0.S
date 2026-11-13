.section .text.start,"ax",@progbits
.global _start
.type _start,@function
_start:
    xor %rbp, %rbp
    mov (%rsp), %rdi
    lea 8(%rsp), %rsi
    and $-16, %rsp
    call main
    mov %rax, %rdi
    xor %rax, %rax
    int $0x80
1:
    pause
    jmp 1b
.size _start, .-_start

.section .note.GNU-stack,"",@progbits
