OUTPUT_FORMAT(elf64-x86-64)
ENTRY(_start)

PHDRS
{
    text PT_LOAD FLAGS(5);
    data PT_LOAD FLAGS(6);
    tls PT_TLS FLAGS(4);
}

SECTIONS
{
    . = 0x400000;
    .text : ALIGN(16) {
        KEEP(*(.text.start))
        *(.text .text.*)
        *(.rodata .rodata.*)
    } :text

    . = ALIGN(0x1000);
    .tdata : ALIGN(16) {
        __oct_tls_start = .;
        *(.tdata .tdata.*)
    } :data :tls

    .tbss (NOLOAD) : ALIGN(16) {
        *(.tbss .tbss.*)
        __oct_tls_end = .;
    } :data :tls

    . = __oct_tls_end;
    .data : {
        *(.data .data.*)
        *(.data.rel.ro .data.rel.ro.*)
        *(.got .got.*)
        . = ALIGN(8);
        __oct_tls_filesz = .;
        QUAD(SIZEOF(.tdata))
        __oct_tls_size = .;
        QUAD(__oct_tls_end - __oct_tls_start)
        __oct_tls_align = .;
        QUAD(MAX(ALIGNOF(.tdata), ALIGNOF(.tbss)))
    } :data

    .bss : {
        *(COMMON)
        *(.bss .bss.*)
    } :data

    /DISCARD/ : {
        *(.comment)
        *(.note*)
        *(.eh_frame*)
    }
}
