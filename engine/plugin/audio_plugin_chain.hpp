#pragma once
#include "../../core/include/constants.hpp"
#include "../../core/include/settings_manager.hpp"
#include "audio_plugin_host.hpp"
#include <memory>
#include <mutex>
#include <optional>
#include <vector>

namespace AviQtl::Engine::Plugin {

class AudioPluginChain {
  public:
    AudioPluginChain() {
        const auto &sm = AviQtl::Core::SettingsManager::instance();
        m_sampleRate = sm.doubleValue(QStringLiteral("defaultProjectSampleRate"),
                                      AviQtl::kDefaultSampleRate);
        m_maxBlockSize = sm.intValue(QStringLiteral("audioPluginMaxBlockSize"),
                                     AviQtl::kAudioMaxBlockSize);
    }
    AudioPluginChain(double sampleRate, int maxBlockSize)
        : m_sampleRate(sampleRate), m_maxBlockSize(maxBlockSize) {}

    struct Description {
        QString name;
        QString format;
        std::vector<ParamInfo> parameters;
        std::vector<float> values;
    };

    void add(std::unique_ptr<IAudioPlugin> plugin, bool enabled = true);
    void clear();
    void prepare(double sampleRate);
    // mix() から呼ばれる：バッファをチェーン内の全プラグインに通す
    void process(float *buf, int frameCount);

    int count() const;
    std::optional<Description> describe(int index) const;
    bool setParameter(int pluginIndex, int parameterIndex, float value);

  private:
    struct Entry {
        std::unique_ptr<IAudioPlugin> plugin;
        bool enabled = true;
    };

    std::vector<Entry> m_plugins;
    mutable std::mutex m_mutex;
    double m_sampleRate = static_cast<double>(AviQtl::kDefaultSampleRate);
    int m_maxBlockSize = AviQtl::kAudioMaxBlockSize;
};

} // namespace AviQtl::Engine::Plugin
