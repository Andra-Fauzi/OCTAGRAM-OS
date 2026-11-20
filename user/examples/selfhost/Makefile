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

.PHONY: order-only-check-run
order-only-check-run: order-only-check

order-only-check: message.o | order-only-dep
	$(CC) -c message.c -o order-only-check

order-only-dep:
	$(CC) -c message.c -o order-only-dep
