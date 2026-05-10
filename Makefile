BINARY_NAME=xui-offline-builder
TEST_DIR=tests
TEST_BIN=$(TEST_DIR)/$(BINARY_NAME)_test

.PHONY: all build test clean

all: build

build:
	@echo "Building for Linux..."
	cargo build

test: build
	@echo "Preparing test environment..."
	mkdir -p $(TEST_DIR)
	cp target/debug/$(BINARY_NAME) $(TEST_BIN)
	@echo "Running test binary..."
	./$(TEST_BIN)

clean:
	cargo clean
	rm -rf $(TEST_DIR)
