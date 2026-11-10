# Host tests for the nixxa protocol core: make -C firmware/nixxa_esp32/tests
CC ?= cc
WARN := -std=c99 -Wall -Wextra -Wpedantic -Wshadow -Werror
SAN := -fsanitize=address,undefined -fno-sanitize-recover=all -fno-omit-frame-pointer
SRC := test_core.c ../nx_core.c ../nx_ancs.c ../nx_sha256.c ../qrcodegen.c
CLOCK_SRC := test_clock.c ../nx_clock.c
B ?= $(or $(TMPDIR),/tmp)/nixxa-core-tests

.PHONY: test clean
test: | $(B)
	gcc $(WARN) -O1 -g $(SAN) -o $(B)/test_core $(SRC)
	$(B)/test_core
	gcc $(WARN) -O1 -g $(SAN) -o $(B)/test_clock $(CLOCK_SRC)
	$(B)/test_clock

$(B):
	mkdir -p $(B)

clean:
	rm -rf $(B)
