#pragma once

#include "rust_core_abi.hpp"
#include <QByteArray>
#include <QString>
#include <QVariant>
#include <QVariantMap>
#include <cstdint>

namespace AviQtl::RustCore::Settings {

enum class Status : std::uint32_t {
    Ok = AVIQTL_RUST_CORE_STATUS_OK,
    InvalidArgument = AVIQTL_RUST_CORE_STATUS_INVALID_ARGUMENT,
    OverlappingBuffers = AVIQTL_RUST_CORE_STATUS_OVERLAPPING_BUFFERS,
    BufferTooSmall = AVIQTL_RUST_CORE_STATUS_BUFFER_TOO_SMALL,
    InvalidJson = AVIQTL_RUST_CORE_STATUS_INVALID_JSON,
};

class State final {
  public:
    State() = default;
    State(const State &) = delete;
    State &operator=(const State &) = delete;
    State(State &&other) noexcept;
    State &operator=(State &&other) noexcept;
    ~State();

    [[nodiscard]] bool isValid() const { return m_handle != nullptr; }
    [[nodiscard]] Status initializeDefaults(const QVariantMap &platformDefaults);
    [[nodiscard]] Status reset(const QVariantMap &settings);
    [[nodiscard]] Status mergeLoaded(const QByteArray &loaded, bool &migrated);
    [[nodiscard]] Status snapshot(QVariantMap &settings) const;
    [[nodiscard]] Status persistentJson(QByteArray &output) const;
    [[nodiscard]] Status setValue(const QString &key, const QVariant &value, bool &changed,
                                  bool &persistent);
    [[nodiscard]] Status removeValue(const QString &key, bool &changed, bool &persistent);
    [[nodiscard]] int intValue(const QString &key, int fallback) const;
    [[nodiscard]] double doubleValue(const QString &key, double fallback) const;
    [[nodiscard]] bool boolValue(const QString &key, bool fallback) const;

  private:
    AviQtlSettingsState *m_handle = nullptr;
};

} // namespace AviQtl::RustCore::Settings
