CC = tcc
TARGET = hello.elf
OBJECTS = hello.o message.o

.PHONY: all
all: $(TARGET)

hello.o: hello.c
	$(CC) -c $< -o $@

message.o: message.c
	$(CC) -c $< -o $@

$(TARGET): $(OBJECTS)
	$(CC) $^ -o $@
