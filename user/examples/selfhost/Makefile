include selfhost-rules.mk

CC = tcc
TARGET = hello.elf
ARCHIVE = libmessage.a

.PHONY: all phony-check
all: $(TARGET)


$(ARCHIVE): message.o
	$(CC) -ar rcs $@ $<

$(TARGET): hello.o $(ARCHIVE)
	$(CC) $^ -o $@

phony-check:
	$(CC) -c message.c -o phony-check
