/**
 * @file raya.h
 * @brief C API for the Raya Virtual Machine
 *
 * This header provides a C-compatible API for embedding the Raya VM in other languages.
 *
 * ## Thread Safety
 * All VM instances are thread-safe and can be used from multiple threads.
 *
 * ## Memory Management
 * - All `raya_vm_*` functions that return pointers transfer ownership to the caller
 * - Caller must free returned resources using the appropriate `*_free()` functions
 * - NULL pointers are safe to pass to `*_free()` functions (no-op)
 *
 * ## Error Handling
 * - Most functions accept an optional `RayaError**` parameter for error reporting
 * - If an error occurs, the function returns an error code (typically -1 or NULL)
 * - Retrieve error message with `raya_error_message()`
 * - Always free errors with `raya_error_free()`
 *
 * ## Example
 * ```c
 * // Create VM
 * RayaError* error = NULL;
 * RayaVM* vm = raya_vm_new(&error);
 * if (vm == NULL) {
 *     fprintf(stderr, "Error: %s\n", raya_error_message(error));
 *     raya_error_free(error);
 *     return 1;
 * }
 *
 * // Load bytecode
 * if (raya_vm_load_file(vm, "./program.rbin", &error) != 0) {
 *     fprintf(stderr, "Error: %s\n", raya_error_message(error));
 *     raya_error_free(error);
 *     raya_vm_destroy(vm);
 *     return 1;
 * }
 *
 * // Run entry point
 * if (raya_vm_run_entry(vm, "main", &error) != 0) {
 *     fprintf(stderr, "Error: %s\n", raya_error_message(error));
 *     raya_error_free(error);
 *     raya_vm_destroy(vm);
 *     return 1;
 * }
 *
 * // Cleanup
 * raya_vm_destroy(vm);
 * ```
 *
 * @version 0.1.0
 * @author Raya Contributors
 * @license MIT OR Apache-2.0
 */

#ifndef RAYA_H
#define RAYA_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ============================================================================
 * Version Information
 * ========================================================================= */

/** Major version (breaking changes) */
#define RAYA_VERSION_MAJOR 0

/** Minor version (new features, backward compatible) */
#define RAYA_VERSION_MINOR 1

/** Patch version (bug fixes) */
#define RAYA_VERSION_PATCH 0

/* ============================================================================
 * Opaque Types
 * ========================================================================= */

/**
 * @brief Opaque handle to a Raya VM instance
 *
 * Represents a complete VM runtime with its own heap, task scheduler,
 * and resource limits. Multiple VMs can coexist independently.
 */
typedef struct RayaVM RayaVM;

/**
 * @brief Opaque handle to a Raya value
 *
 * Represents a runtime value (null, boolean, integer, string, object, etc.).
 */
typedef struct RayaValue RayaValue;

/**
 * @brief Opaque handle to a VM snapshot
 *
 * Contains the complete serialized state of a VM, including heap,
 * task state, and globals. Can be used to save/restore VM state.
 */
typedef struct RayaSnapshot RayaSnapshot;

/**
 * @brief Error information
 *
 * Contains error message and optional details. Always free with
 * `raya_error_free()` after use.
 */
typedef struct RayaError RayaError;

/* ============================================================================
 * VM Lifecycle Functions
 * ========================================================================= */

/**
 * @brief Create a new Raya VM instance
 *
 * Creates a VM with default configuration (no resource limits).
 *
 * @param error Optional pointer to receive error information (may be NULL)
 * @return Pointer to RayaVM on success, NULL on failure
 *
 * @note The returned VM must be freed with `raya_vm_destroy()`
 * @see raya_vm_destroy
 */
RayaVM* raya_vm_new(RayaError** error);

/**
 * @brief Destroy a Raya VM instance and free all resources
 *
 * Terminates all running tasks and releases all memory.
 * Safe to call multiple times with the same VM (idempotent).
 *
 * @param vm Pointer to RayaVM (may be NULL)
 *
 * @warning VM must not be used after this call
 */
void raya_vm_destroy(RayaVM* vm);

/**
 * @brief Load a .rbin bytecode file into the VM
 *
 * Loads and validates a compiled Raya bytecode module.
 *
 * @param vm Pointer to RayaVM (must not be NULL)
 * @param path Null-terminated path to .rbin file
 * @param error Optional pointer to receive error information
 * @return 0 on success, -1 on failure
 *
 * @see raya_vm_load_bytes
 */
int raya_vm_load_file(RayaVM* vm, const char* path, RayaError** error);

/**
 * @brief Load bytecode from memory
 *
 * Loads and validates bytecode from a memory buffer.
 *
 * @param vm Pointer to RayaVM (must not be NULL)
 * @param bytes Pointer to bytecode data
 * @param length Length of bytecode data in bytes
 * @param error Optional pointer to receive error information
 * @return 0 on success, -1 on failure
 *
 * @see raya_vm_load_file
 */
int raya_vm_load_bytes(RayaVM* vm, const uint8_t* bytes, size_t length, RayaError** error);

/**
 * @brief Run an entry point function
 *
 * Executes the specified function (typically "main"). Blocks until
 * the function completes.
 *
 * @param vm Pointer to RayaVM (must not be NULL)
 * @param name Null-terminated function name (e.g., "main")
 * @param error Optional pointer to receive error information
 * @return 0 on success, -1 on failure
 */
int raya_vm_run_entry(RayaVM* vm, const char* name, RayaError** error);

/**
 * @brief Terminate the VM and stop all running tasks
 *
 * Stops all running tasks but does not destroy the VM. The VM can
 * be reused by loading new code.
 *
 * @param vm Pointer to RayaVM (must not be NULL)
 * @param error Optional pointer to receive error information
 * @return 0 on success, -1 on failure
 */
int raya_vm_terminate(RayaVM* vm, RayaError** error);

/* ============================================================================
 * Value Creation Functions
 * ========================================================================= */

/**
 * @brief Create a null value
 *
 * @return Pointer to RayaValue representing null
 * @note The returned value must be freed with `raya_value_free()`
 */
RayaValue* raya_value_null(void);

/**
 * @brief Create a boolean value
 *
 * @param value Boolean value (0 = false, non-zero = true)
 * @return Pointer to RayaValue representing the boolean
 * @note The returned value must be freed with `raya_value_free()`
 */
RayaValue* raya_value_bool(int value);

/**
 * @brief Create a 32-bit integer value
 *
 * @param value Integer value
 * @return Pointer to RayaValue representing the integer
 * @note The returned value must be freed with `raya_value_free()`
 */
RayaValue* raya_value_i32(int32_t value);

/**
 * @brief Free a value
 *
 * Releases the memory associated with a value. Safe to call with NULL.
 *
 * @param value Pointer to RayaValue (may be NULL)
 * @warning Value must not be used after this call
 */
void raya_value_free(RayaValue* value);

/* ============================================================================
 * Error Handling Functions
 * ========================================================================= */

/**
 * @brief Get the error message
 *
 * Returns a null-terminated string describing the error.
 *
 * @param error Pointer to RayaError (must not be NULL)
 * @return Null-terminated error message, or NULL if error is NULL
 *
 * @note The returned string is valid until `raya_error_free()` is called
 * @warning Do not free the returned string directly
 */
const char* raya_error_message(const RayaError* error);

/**
 * @brief Free an error
 *
 * Releases the memory associated with an error. Safe to call with NULL.
 *
 * @param error Pointer to RayaError (may be NULL)
 * @warning Error must not be used after this call
 */
void raya_error_free(RayaError* error);

/* ============================================================================
 * Snapshot Functions
 * ========================================================================= */

/**
 * @brief Create a snapshot of the VM state
 *
 * Captures the complete state of the VM including heap, tasks, and globals.
 * The VM is paused during snapshotting.
 *
 * @param vm Pointer to RayaVM (must not be NULL)
 * @param error Optional pointer to receive error information
 * @return Pointer to RayaSnapshot on success, NULL on failure
 *
 * @note The returned snapshot must be freed with `raya_snapshot_free()`
 * @see raya_vm_restore, raya_snapshot_free
 */
RayaSnapshot* raya_vm_snapshot(RayaVM* vm, RayaError** error);

/**
 * @brief Restore VM state from a snapshot
 *
 * Replaces the current VM state with the state from the snapshot.
 * The snapshot is consumed and must not be used after this call.
 *
 * @param vm Pointer to RayaVM (must not be NULL)
 * @param snapshot Pointer to RayaSnapshot (must not be NULL, consumed)
 * @param error Optional pointer to receive error information
 * @return 0 on success, -1 on failure
 *
 * @warning The snapshot is consumed and must not be used after this call
 * @see raya_vm_snapshot
 */
int raya_vm_restore(RayaVM* vm, RayaSnapshot* snapshot, RayaError** error);

/**
 * @brief Free a snapshot
 *
 * Releases the memory associated with a snapshot. Safe to call with NULL.
 *
 * @param snapshot Pointer to RayaSnapshot (may be NULL)
 * @warning Snapshot must not be used after this call
 */
void raya_snapshot_free(RayaSnapshot* snapshot);

/* ============================================================================
 * Version Information
 * ========================================================================= */

/**
 * @brief Get the Raya VM version string
 *
 * Returns the version in the format "MAJOR.MINOR.PATCH" (e.g., "0.1.0").
 *
 * @return Null-terminated version string
 * @note The returned string is a static string and must not be freed
 */
const char* raya_version(void);

#ifdef __cplusplus
}
#endif

#endif /* RAYA_H */
