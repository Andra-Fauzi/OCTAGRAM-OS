CC = tcc
TARGET = hello.elf
ARCHIVE = libmessage.a

.PHONY: all phony-check
all: $(TARGET)

%.o: %.c
	$(CC) -c $< -o $*.o

$(ARCHIVE): message.o
	$(CC) -ar rcs $@ $<

$(TARGET): hello.o $(ARCHIVE)
	$(CC) $^ -o $@

phony-check:
	$(CC) -c message.c -o phony-check
