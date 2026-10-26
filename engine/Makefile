# Host build for the DeskBunny engine: tests, renderer, contact sheet.
#   make test     build + run tests with gcc and clang (ASan + UBSan)
#   make sheet    build/moods.pbm and build/moods.png (contact sheet of every mood)
#   make render   build/render
CC ?= cc
PYTHON ?= python3
WARN := -std=c99 -Wall -Wextra -Wpedantic -Wconversion -Wshadow -Werror
SAN := -fsanitize=address,undefined -fno-sanitize-recover=all -fno-omit-frame-pointer
B := build

.PHONY: all test sheet render clean

all: test sheet

$(B):
	mkdir -p $(B)

$(B)/render: tools/render.c deskbunny_engine.c deskbunny_engine.h | $(B)
	$(CC) $(WARN) -O2 -o $@ tools/render.c deskbunny_engine.c

render: $(B)/render

test: | $(B)
	gcc $(WARN) -O1 -g $(SAN) -o $(B)/test_gcc tests/test_engine.c deskbunny_engine.c
	./$(B)/test_gcc
	clang $(WARN) -O1 -g $(SAN) -o $(B)/test_clang tests/test_engine.c deskbunny_engine.c
	./$(B)/test_clang

sheet: $(B)/render
	./$(B)/render sheet $(B)/moods.pbm
	$(PYTHON) tools/pbm2png.py $(B)/moods.pbm $(B)/moods.png 2

clean:
	rm -rf $(B)
