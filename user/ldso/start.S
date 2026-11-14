.section .text.start,"ax",@progbits
.global _start
.type _start,@function
_start:
    mov %rsp, %rdi
    and $-16, %rsp
    call oct_ldso_main
1:
    pause
    jmp 1b
.size _start, .-_start

.section .note.GNU-stack,"",@progbits
