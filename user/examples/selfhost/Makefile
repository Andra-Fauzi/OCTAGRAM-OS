CC = tcc
TARGET = hello.elf

.PHONY: all
all: $(TARGET)

$(TARGET): hello.c message.c
	$(CC) $^ -o $@
