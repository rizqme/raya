/**
 * @file fs.cpp
 * @brief Modern C++ native module example for file system operations
 *
 * This demonstrates advanced C++ patterns for native modules:
 * - RAII wrappers
 * - std::optional for error handling
 * - Template metaprogramming for type conversion
 * - Lambda-based error handling
 *
 * Raya usage:
 * ```typescript
 * import { readFile, writeFile, exists, mkdir } from "native:fs";
 *
 * if (exists("/tmp/test.txt")) {
 *     const content = readFile("/tmp/test.txt");
 *     console.log(content);
 * }
 *
 * writeFile("/tmp/output.txt", "Hello, world!");
 * mkdir("/tmp/mydir");
 * ```
 *
 * Build:
 *   g++ -std=c++17 -fPIC -shared -o libfs.so fs.cpp -I../include -lstdc++fs
 */

#include <raya/module.h>
#include <string>
#include <fstream>
#include <filesystem>
#include <optional>
#include <variant>
#include <memory>

namespace fs = std::filesystem;

// ============================================================================
// C++ Wrapper API for Raya Values
// ============================================================================

namespace raya {

/**
 * RAII wrapper for RayaValue with automatic lifetime management
 */
class Value {
    RayaContext* ctx_;
    RayaValue* value_;

public:
    Value(RayaContext* ctx, RayaValue* value)
        : ctx_(ctx), value_(value) {}

    // Move-only type (no copying)
    Value(const Value&) = delete;
    Value& operator=(const Value&) = delete;
    Value(Value&& other) noexcept
        : ctx_(other.ctx_), value_(other.value_) {
        other.value_ = nullptr;
    }

    RayaValue* release() {
        auto v = value_;
        value_ = nullptr;
        return v;
    }

    RayaValue* get() const { return value_; }
};

/**
 * Result type for operations that can fail
 */
template<typename T>
using Result = std::variant<T, std::string>;

/**
 * Type conversion traits for automatic marshalling
 */
template<typename T>
struct TypeConverter;

// Specialization for string
template<>
struct TypeConverter<std::string> {
    static std::string from_raya(RayaValue* value) {
        const char* str = raya_value_to_string(value);
        return str ? std::string(str) : std::string();
    }

    static RayaValue* to_raya(RayaContext* ctx, const std::string& value) {
        return raya_value_from_string(ctx, value.c_str());
    }
};

// Specialization for bool
template<>
struct TypeConverter<bool> {
    static bool from_raya(RayaValue* value) {
        return raya_value_to_bool(value) != 0;
    }

    static RayaValue* to_raya(RayaContext* ctx, bool value) {
        return raya_value_from_bool(ctx, value ? 1 : 0);
    }
};

// Specialization for int32_t
template<>
struct TypeConverter<int32_t> {
    static int32_t from_raya(RayaValue* value) {
        return raya_value_to_i32(value);
    }

    static RayaValue* to_raya(RayaContext* ctx, int32_t value) {
        return raya_value_from_i32(ctx, value);
    }
};

/**
 * Helper to create error values
 */
inline RayaValue* error(RayaContext* ctx, const std::string& message) {
    return raya_error_new(ctx, message.c_str());
}

} // namespace raya

// ============================================================================
// File System Operations
// ============================================================================

namespace fs_ops {

/**
 * Read entire file as string
 */
raya::Result<std::string> read_file(const std::string& path) {
    try {
        std::ifstream file(path);
        if (!file.is_open()) {
            return std::string("Failed to open file: ") + path;
        }

        std::string content(
            (std::istreambuf_iterator<char>(file)),
            std::istreambuf_iterator<char>()
        );

        return content;
    } catch (const std::exception& e) {
        return std::string("Error reading file: ") + e.what();
    }
}

/**
 * Write string to file
 */
raya::Result<bool> write_file(const std::string& path, const std::string& content) {
    try {
        std::ofstream file(path);
        if (!file.is_open()) {
            return std::string("Failed to open file for writing: ") + path;
        }

        file << content;
        if (file.fail()) {
            return std::string("Failed to write to file: ") + path;
        }

        return true;
    } catch (const std::exception& e) {
        return std::string("Error writing file: ") + e.what();
    }
}

/**
 * Check if file or directory exists
 */
bool exists(const std::string& path) {
    return fs::exists(path);
}

/**
 * Create directory (with parents)
 */
raya::Result<bool> mkdir(const std::string& path) {
    try {
        std::error_code ec;
        bool created = fs::create_directories(path, ec);
        if (ec) {
            return std::string("Failed to create directory: ") + ec.message();
        }
        return created;
    } catch (const std::exception& e) {
        return std::string("Error creating directory: ") + e.what();
    }
}

/**
 * Remove file or directory
 */
raya::Result<bool> remove(const std::string& path) {
    try {
        std::error_code ec;
        bool removed = fs::remove(path, ec);
        if (ec) {
            return std::string("Failed to remove: ") + ec.message();
        }
        return removed;
    } catch (const std::exception& e) {
        return std::string("Error removing: ") + e.what();
    }
}

/**
 * List directory contents
 */
raya::Result<std::vector<std::string>> list_dir(const std::string& path) {
    try {
        std::vector<std::string> entries;
        for (const auto& entry : fs::directory_iterator(path)) {
            entries.push_back(entry.path().filename().string());
        }
        return entries;
    } catch (const std::exception& e) {
        return std::string("Error listing directory: ") + e.what();
    }
}

} // namespace fs_ops

// ============================================================================
// Native Function Wrappers (Bridge between Raya and C++)
// ============================================================================

/**
 * Helper macro for implementing native functions with automatic type conversion
 */
#define RAYA_FUNCTION(name, impl)                                             \
    RayaValue* native_##name(RayaContext* ctx, RayaValue** args, size_t argc)

/**
 * readFile(path: string): string
 */
RAYA_FUNCTION(read_file, {
    if (argc != 1) {
        return raya::error(ctx, "readFile() requires 1 argument");
    }

    std::string path = raya::TypeConverter<std::string>::from_raya(args[0]);
    auto result = fs_ops::read_file(path);

    return std::visit([ctx](auto&& value) -> RayaValue* {
        using T = std::decay_t<decltype(value)>;
        if constexpr (std::is_same_v<T, std::string>) {
            // Check if it's an error (we use convention: error strings start differently)
            // In real implementation, we'd use Result<T,E> properly
            if (value.find("Error") == 0 || value.find("Failed") == 0) {
                return raya::error(ctx, value);
            }
            return raya::TypeConverter<std::string>::to_raya(ctx, value);
        }
        return raya::error(ctx, "Internal error");
    }, result);
});

/**
 * writeFile(path: string, content: string): void
 */
RAYA_FUNCTION(write_file, {
    if (argc != 2) {
        return raya::error(ctx, "writeFile() requires 2 arguments");
    }

    std::string path = raya::TypeConverter<std::string>::from_raya(args[0]);
    std::string content = raya::TypeConverter<std::string>::from_raya(args[1]);

    auto result = fs_ops::write_file(path, content);

    return std::visit([ctx](auto&& value) -> RayaValue* {
        using T = std::decay_t<decltype(value)>;
        if constexpr (std::is_same_v<T, bool>) {
            return raya_value_null(ctx); // Success returns null (void)
        } else {
            return raya::error(ctx, value);
        }
    }, result);
});

/**
 * exists(path: string): boolean
 */
RAYA_FUNCTION(exists, {
    if (argc != 1) {
        return raya::error(ctx, "exists() requires 1 argument");
    }

    std::string path = raya::TypeConverter<std::string>::from_raya(args[0]);
    bool result = fs_ops::exists(path);

    return raya::TypeConverter<bool>::to_raya(ctx, result);
});

/**
 * mkdir(path: string): void
 */
RAYA_FUNCTION(mkdir, {
    if (argc != 1) {
        return raya::error(ctx, "mkdir() requires 1 argument");
    }

    std::string path = raya::TypeConverter<std::string>::from_raya(args[0]);
    auto result = fs_ops::mkdir(path);

    return std::visit([ctx](auto&& value) -> RayaValue* {
        using T = std::decay_t<decltype(value)>;
        if constexpr (std::is_same_v<T, bool>) {
            return raya_value_null(ctx);
        } else {
            return raya::error(ctx, value);
        }
    }, result);
});

/**
 * remove(path: string): boolean
 */
RAYA_FUNCTION(remove, {
    if (argc != 1) {
        return raya::error(ctx, "remove() requires 1 argument");
    }

    std::string path = raya::TypeConverter<std::string>::from_raya(args[0]);
    auto result = fs_ops::remove(path);

    return std::visit([ctx](auto&& value) -> RayaValue* {
        using T = std::decay_t<decltype(value)>;
        if constexpr (std::is_same_v<T, bool>) {
            return raya::TypeConverter<bool>::to_raya(ctx, value);
        } else {
            return raya::error(ctx, value);
        }
    }, result);
});

/**
 * listDir(path: string): string[]
 */
RAYA_FUNCTION(list_dir, {
    if (argc != 1) {
        return raya::error(ctx, "listDir() requires 1 argument");
    }

    std::string path = raya::TypeConverter<std::string>::from_raya(args[0]);
    auto result = fs_ops::list_dir(path);

    return std::visit([ctx](auto&& value) -> RayaValue* {
        using T = std::decay_t<decltype(value)>;
        if constexpr (std::is_same_v<T, std::vector<std::string>>) {
            // Create Raya array
            RayaValue* array = raya_array_new(ctx, value.size());
            for (size_t i = 0; i < value.size(); i++) {
                raya_array_set(array, i,
                    raya::TypeConverter<std::string>::to_raya(ctx, value[i]));
            }
            return array;
        } else {
            return raya::error(ctx, value);
        }
    }, result);
});

// ============================================================================
// Module Registration
// ============================================================================

extern "C" {

RAYA_MODULE_INIT(fs) {
    RayaModuleBuilder* builder = raya_module_builder_new("fs", "1.0.0");

    raya_module_add_function(builder, "readFile", native_read_file, 1);
    raya_module_add_function(builder, "writeFile", native_write_file, 2);
    raya_module_add_function(builder, "exists", native_exists, 1);
    raya_module_add_function(builder, "mkdir", native_mkdir, 1);
    raya_module_add_function(builder, "remove", native_remove, 1);
    raya_module_add_function(builder, "listDir", native_list_dir, 1);

    return raya_module_builder_finish(builder);
}

} // extern "C"

// ============================================================================
// Usage Example
// ============================================================================

/*
// Raya program (fs_example.raya)
import { readFile, writeFile, exists, mkdir, remove, listDir } from "native:fs";

// Create directory
mkdir("/tmp/raya_test");

// Write file
writeFile("/tmp/raya_test/hello.txt", "Hello from Raya!");

// Check if file exists
if (exists("/tmp/raya_test/hello.txt")) {
    // Read file
    const content = readFile("/tmp/raya_test/hello.txt");
    console.log("File content:", content);
}

// List directory
const files = listDir("/tmp/raya_test");
console.log("Files:", files);

// Clean up
remove("/tmp/raya_test/hello.txt");
remove("/tmp/raya_test");
*/
