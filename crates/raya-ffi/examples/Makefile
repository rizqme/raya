# Makefile for building Raya native modules in C/C++
#
# Usage:
#   make all          - Build all examples
#   make crypto       - Build crypto module
#   make fs           - Build fs module
#   make clean        - Remove build artifacts
#   make install      - Install modules to ~/.raya/modules

# Compiler settings
CC = gcc
CXX = g++
CFLAGS = -fPIC -O2 -Wall -Wextra -I../include
CXXFLAGS = -std=c++17 -fPIC -O2 -Wall -Wextra -I../include
LDFLAGS = -shared

# Platform-specific settings
UNAME_S := $(shell uname -s)
ifeq ($(UNAME_S),Linux)
    EXT = so
    CRYPTO_LIBS = -lcrypto
    FS_LIBS = -lstdc++fs
endif
ifeq ($(UNAME_S),Darwin)
    EXT = dylib
    CRYPTO_LIBS = -lcrypto
    FS_LIBS =
endif

# Output directory
OUTDIR = build
INSTALL_DIR = $(HOME)/.raya/modules

# Targets
CRYPTO_MODULE = $(OUTDIR)/libcrypto.$(EXT)
FS_MODULE = $(OUTDIR)/libfs.$(EXT)
HELLO_EXAMPLE = $(OUTDIR)/hello

.PHONY: all clean install crypto fs hello

all: $(CRYPTO_MODULE) $(FS_MODULE) $(HELLO_EXAMPLE)

# Create output directory
$(OUTDIR):
	mkdir -p $(OUTDIR)

# Build crypto module (C++)
$(CRYPTO_MODULE): crypto.cpp | $(OUTDIR)
	$(CXX) $(CXXFLAGS) $(LDFLAGS) -o $@ $< $(CRYPTO_LIBS)
	@echo "✓ Built crypto module: $@"

# Build fs module (C++)
$(FS_MODULE): fs.cpp | $(OUTDIR)
	$(CXX) $(CXXFLAGS) $(LDFLAGS) -o $@ $< $(FS_LIBS)
	@echo "✓ Built fs module: $@"

# Build hello example (C)
$(HELLO_EXAMPLE): hello.c | $(OUTDIR)
	$(CC) $(CFLAGS) -o $@ $< -L../../target/debug -lraya_ffi
	@echo "✓ Built hello example: $@"

# Shorthand targets
crypto: $(CRYPTO_MODULE)
fs: $(FS_MODULE)
hello: $(HELLO_EXAMPLE)

# Install modules
install: $(CRYPTO_MODULE) $(FS_MODULE)
	@mkdir -p $(INSTALL_DIR)
	cp $(CRYPTO_MODULE) $(INSTALL_DIR)/
	cp $(FS_MODULE) $(INSTALL_DIR)/
	@echo "✓ Installed modules to $(INSTALL_DIR)"

# Clean build artifacts
clean:
	rm -rf $(OUTDIR)
	@echo "✓ Cleaned build artifacts"

# Help
help:
	@echo "Raya Native Module Builder"
	@echo ""
	@echo "Targets:"
	@echo "  all      - Build all examples"
	@echo "  crypto   - Build crypto module (SHA256, random bytes)"
	@echo "  fs       - Build filesystem module (read/write files)"
	@echo "  hello    - Build hello example (C API usage)"
	@echo "  install  - Install modules to ~/.raya/modules"
	@echo "  clean    - Remove build artifacts"
	@echo ""
	@echo "Examples:"
	@echo "  make crypto              # Build crypto module"
	@echo "  make install             # Install all modules"
	@echo "  make clean && make all   # Clean rebuild"
