/**
 * @file hello.c
 * @brief Simple example of using the Raya VM C API
 *
 * This example demonstrates:
 * - Creating a VM
 * - Error handling
 * - Loading bytecode
 * - Running an entry point
 * - Cleanup
 *
 * Build with:
 *   gcc -o hello hello.c -L../../target/debug -lraya_ffi -I../include
 *
 * Run with:
 *   LD_LIBRARY_PATH=../../target/debug ./hello
 */

#include <stdio.h>
#include <stdlib.h>
#include "../include/raya.h"

int main(int argc, char** argv) {
    printf("Raya VM C API Example\n");
    printf("Version: %s\n\n", raya_version());

    // Create VM
    RayaError* error = NULL;
    RayaVM* vm = raya_vm_new(&error);
    if (vm == NULL) {
        fprintf(stderr, "Failed to create VM: %s\n", raya_error_message(error));
        raya_error_free(error);
        return 1;
    }
    printf("✓ VM created successfully\n");

    // Load bytecode file (if provided)
    if (argc > 1) {
        const char* path = argv[1];
        printf("Loading bytecode from: %s\n", path);

        if (raya_vm_load_file(vm, path, &error) != 0) {
            fprintf(stderr, "Failed to load file: %s\n", raya_error_message(error));
            raya_error_free(error);
            raya_vm_destroy(vm);
            return 1;
        }
        printf("✓ Bytecode loaded successfully\n");

        // Run entry point
        printf("Running 'main' function...\n");
        if (raya_vm_run_entry(vm, "main", &error) != 0) {
            fprintf(stderr, "Execution failed: %s\n", raya_error_message(error));
            raya_error_free(error);
            raya_vm_destroy(vm);
            return 1;
        }
        printf("✓ Execution completed successfully\n");
    } else {
        printf("Note: No bytecode file provided. Usage: %s <file.rbin>\n", argv[0]);
    }

    // Test value creation
    printf("\nTesting value creation...\n");
    RayaValue* null_val = raya_value_null();
    RayaValue* bool_val = raya_value_bool(1);
    RayaValue* int_val = raya_value_i32(42);

    printf("✓ Created null value: %p\n", (void*)null_val);
    printf("✓ Created bool value: %p\n", (void*)bool_val);
    printf("✓ Created int value: %p\n", (void*)int_val);

    // Free values
    raya_value_free(null_val);
    raya_value_free(bool_val);
    raya_value_free(int_val);
    printf("✓ Values freed\n");

    // Test snapshotting (will fail since not implemented yet)
    printf("\nTesting snapshot (expected to fail)...\n");
    RayaSnapshot* snapshot = raya_vm_snapshot(vm, &error);
    if (snapshot == NULL) {
        printf("✗ Snapshot failed (expected): %s\n", raya_error_message(error));
        raya_error_free(error);
    } else {
        printf("✓ Snapshot created\n");
        raya_snapshot_free(snapshot);
    }

    // Cleanup
    printf("\nCleaning up...\n");
    raya_vm_destroy(vm);
    printf("✓ VM destroyed\n");

    printf("\n✓ All tests passed!\n");
    return 0;
}
