.text
.global ldso_transfer
.type ldso_transfer,@function
ldso_transfer:
    mov %rsi, %rsp
    xor %rbp, %rbp
    jmp *%rdi
.size ldso_transfer, .-ldso_transfer

.section .note.GNU-stack,"",@progbits
