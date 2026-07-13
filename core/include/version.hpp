#pragma once

namespace AviQtl {
// Defined by CMake (AVIQTL_VERSION_STRING); defaults to "0.0.0" for IDE builds.
#ifndef AVIQTL_VERSION_STRING
constexpr const char *VERSION_STRING = "0.0.0";
#else
constexpr const char *VERSION_STRING = AVIQTL_VERSION_STRING;
#endif

} // namespace AviQtl
