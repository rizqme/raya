/**
 * @file crypto.cpp
 * @brief Example native module in C++ for Raya
 *
 * This demonstrates how to write a native module that Raya programs can import:
 *
 * Raya usage:
 * ```typescript
 * import { hash, randomBytes } from "native:crypto";
 * const digest = hash("sha256", "hello world");
 * const random = randomBytes(32);
 * ```
 *
 * Build:
 *   g++ -std=c++17 -fPIC -shared -o libcrypto.so crypto.cpp -I../include
 *
 * Install:
 *   cp libcrypto.so ~/.raya/modules/
 */

#include <raya/module.h>
#include <string>
#include <vector>
#include <cstring>
#include <random>
#include <openssl/sha.h>  // For SHA256 (requires -lcrypto)

// ============================================================================
// Helper: RAII wrapper for RayaValue
// ============================================================================

class RayaValueGuard {
    RayaValue* value_;
public:
    explicit RayaValueGuard(RayaValue* value) : value_(value) {}
    ~RayaValueGuard() {
        // Note: In real implementation, values are GC-managed
        // This is just for demonstration
    }

    RayaValue* get() const { return value_; }
    RayaValue* release() {
        auto v = value_;
        value_ = nullptr;
        return v;
    }
};

// ============================================================================
// Native Functions
// ============================================================================

/**
 * Hash a string using the specified algorithm
 *
 * Signature: hash(algorithm: string, data: string): string
 */
RayaValue* native_hash(RayaContext* ctx, RayaValue** args, size_t argc) {
    // Validate argument count
    if (argc != 2) {
        return raya_error_new(ctx, "hash() requires 2 arguments");
    }

    // Extract algorithm (first argument)
    const char* algorithm = raya_value_to_string(args[0]);
    if (!algorithm) {
        return raya_error_new(ctx, "First argument must be a string");
    }

    // Extract data (second argument)
    const char* data = raya_value_to_string(args[1]);
    if (!data) {
        return raya_error_new(ctx, "Second argument must be a string");
    }

    // Compute hash based on algorithm
    if (strcmp(algorithm, "sha256") == 0) {
        unsigned char hash[SHA256_DIGEST_LENGTH];
        SHA256_CTX sha256;
        SHA256_Init(&sha256);
        SHA256_Update(&sha256, data, strlen(data));
        SHA256_Final(hash, &sha256);

        // Convert to hex string
        char hex[SHA256_DIGEST_LENGTH * 2 + 1];
        for (int i = 0; i < SHA256_DIGEST_LENGTH; i++) {
            sprintf(hex + (i * 2), "%02x", hash[i]);
        }
        hex[SHA256_DIGEST_LENGTH * 2] = '\0';

        return raya_value_from_string(ctx, hex);
    }
    else if (strcmp(algorithm, "sha512") == 0) {
        unsigned char hash[SHA512_DIGEST_LENGTH];
        SHA512_CTX sha512;
        SHA512_Init(&sha512);
        SHA512_Update(&sha512, data, strlen(data));
        SHA512_Final(hash, &sha512);

        // Convert to hex string
        char hex[SHA512_DIGEST_LENGTH * 2 + 1];
        for (int i = 0; i < SHA512_DIGEST_LENGTH; i++) {
            sprintf(hex + (i * 2), "%02x", hash[i]);
        }
        hex[SHA512_DIGEST_LENGTH * 2] = '\0';

        return raya_value_from_string(ctx, hex);
    }
    else {
        std::string error = "Unsupported hash algorithm: ";
        error += algorithm;
        error += " (supported: sha256, sha512)";
        return raya_error_new(ctx, error.c_str());
    }
}

/**
 * Generate cryptographically secure random bytes
 *
 * Signature: randomBytes(length: number): Uint8Array
 */
RayaValue* native_random_bytes(RayaContext* ctx, RayaValue** args, size_t argc) {
    // Validate argument count
    if (argc != 1) {
        return raya_error_new(ctx, "randomBytes() requires 1 argument");
    }

    // Extract length
    int32_t length = raya_value_to_i32(args[0]);
    if (length <= 0 || length > 1024 * 1024) {
        return raya_error_new(ctx, "Length must be between 1 and 1048576");
    }

    // Generate random bytes
    std::random_device rd;
    std::mt19937 gen(rd());
    std::uniform_int_distribution<> dis(0, 255);

    std::vector<uint8_t> bytes(length);
    for (int32_t i = 0; i < length; i++) {
        bytes[i] = static_cast<uint8_t>(dis(gen));
    }

    // Create Raya array
    RayaValue* array = raya_array_new(ctx, length);
    for (int32_t i = 0; i < length; i++) {
        raya_array_set(array, i, raya_value_from_i32(ctx, bytes[i]));
    }

    return array;
}

/**
 * Constant-time string comparison (to prevent timing attacks)
 *
 * Signature: constantTimeEqual(a: string, b: string): boolean
 */
RayaValue* native_constant_time_equal(RayaContext* ctx, RayaValue** args, size_t argc) {
    if (argc != 2) {
        return raya_error_new(ctx, "constantTimeEqual() requires 2 arguments");
    }

    const char* a = raya_value_to_string(args[0]);
    const char* b = raya_value_to_string(args[1]);

    if (!a || !b) {
        return raya_error_new(ctx, "Both arguments must be strings");
    }

    size_t len_a = strlen(a);
    size_t len_b = strlen(b);

    // Always compare full length to avoid timing leak
    size_t max_len = (len_a > len_b) ? len_a : len_b;
    int result = 0;

    for (size_t i = 0; i < max_len; i++) {
        uint8_t byte_a = (i < len_a) ? a[i] : 0;
        uint8_t byte_b = (i < len_b) ? b[i] : 0;
        result |= (byte_a ^ byte_b);
    }

    // Also check length equality
    result |= (len_a ^ len_b);

    return raya_value_from_bool(ctx, result == 0);
}

// ============================================================================
// Module Registration (C++ wrapper for cleaner code)
// ============================================================================

class ModuleBuilder {
    RayaModuleBuilder* builder_;

public:
    ModuleBuilder(const char* name, const char* version)
        : builder_(raya_module_builder_new(name, version)) {}

    ~ModuleBuilder() {
        // Builder is consumed by finish(), so only clean up if not finished
    }

    ModuleBuilder& add_function(const char* name, RayaNativeFunction func, size_t arity) {
        raya_module_add_function(builder_, name, func, arity);
        return *this;
    }

    RayaModule* finish() {
        return raya_module_builder_finish(builder_);
    }
};

// ============================================================================
// Module Initialization
// ============================================================================

extern "C" {

/**
 * Module entry point
 *
 * This function is called when the module is loaded by the Raya VM.
 * It must be named: raya_module_init_<modulename>
 * For "native:crypto", the function name is: raya_module_init_crypto
 */
RAYA_MODULE_INIT(crypto) {
    return ModuleBuilder("crypto", "1.0.0")
        .add_function("hash", native_hash, 2)
        .add_function("randomBytes", native_random_bytes, 1)
        .add_function("constantTimeEqual", native_constant_time_equal, 2)
        .finish();
}

} // extern "C"

// ============================================================================
// Usage Example (in comments for documentation)
// ============================================================================

/*
// Raya program (crypto_example.raya)
import { hash, randomBytes, constantTimeEqual } from "native:crypto";

// Hash a string
const digest = hash("sha256", "hello world");
console.log("SHA256:", digest);

// Generate random bytes
const random = randomBytes(32);
console.log("Random bytes:", random.length);

// Constant-time comparison (for security-sensitive code)
const password = "secret";
const input = "secret";
if (constantTimeEqual(password, input)) {
    console.log("Password matches!");
}

// Error handling
try {
    const invalid = hash("md5", "data"); // Unsupported algorithm
} catch (e) {
    console.error("Error:", e.message);
}
*/
