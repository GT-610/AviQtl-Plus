# AviQtl Rust core

## C ABI contract

`core/include/rust_core_abi.hpp` is the single raw C ABI surface shared by the C++ application and `aviqtl-rust-core`.

- Callers must compare `aviqtl_core_abi_version()` with `AVIQTL_RUST_CORE_ABI_VERSION` before relying on the ABI and may inspect `aviqtl_core_capabilities()` for optional operations.
- The boundary contains only fixed-layout scalar fields, pointers, and lengths. Qt types never cross it.
- Every buffer is owned by the caller. Rust neither allocates nor frees memory across the boundary.
- A null pointer is accepted only when its matching length is zero. Non-empty buffers must be correctly aligned and valid for the duration of the call.
- Mutable output ranges must not overlap any input or other output range. Violations return `AVIQTL_RUST_CORE_STATUS_OVERLAPPING_BUFFERS` where the operation has a status result.
- Adding or reordering fields, changing a function signature, or changing a discriminant requires an ABI version increment. Additive functions or capabilities that leave existing layouts intact may use a new capability bit.
