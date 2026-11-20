CC = tcc
TARGET = hello.elf
ARCHIVE = libmessage.a

.PHONY: all phony-check
all: $(TARGET)

hello.o: hello.c
	$(CC) -c $< -o $@

message.o: message.c
	$(CC) -c $< -o $@

$(ARCHIVE): message.o
	$(CC) -ar rcs $@ $<

$(TARGET): hello.o $(ARCHIVE)
	$(CC) $^ -o $@

phony-check:
	touch /share/octagram/phony-check
