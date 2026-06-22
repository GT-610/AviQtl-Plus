#pragma once
#include "../../core/include/constants.hpp"
#include "../../core/include/settings_manager.hpp"
#include "audio_plugin_host.hpp"
#include <memory>
#include <vector>

namespace AviQtl::Engine::Plugin {

class AudioPluginChain {
  public:
    AudioPluginChain() {
        const auto &sm = AviQtl::Core::SettingsManager::instance();
        m_sampleRate = sm.value(QStringLiteral("defaultProjectSampleRate"), AviQtl::kDefaultSampleRate).toDouble();
        m_maxBlockSize = sm.value(QStringLiteral("audioPluginMaxBlockSize"), AviQtl::kAudioMaxBlockSize).toInt();
    }

    void add(std::unique_ptr<IAudioPlugin> plugin, bool enabled = true);
    void remove(int index);
    void clear();
    void setEnabled(int index, bool enabled);
    bool isEnabled(int index) const;

    void prepare(double sampleRate, int maxBlockSize);
    // mix() から呼ばれる：バッファをチェーン内の全プラグインに通す
    void process(float *buf, int frameCount);

    int count() const;
    IAudioPlugin *get(int index) const;

  private:
    struct Entry {
        std::unique_ptr<IAudioPlugin> plugin;
        bool enabled = true;
    };

    std::vector<Entry> m_plugins;
    double m_sampleRate = static_cast<double>(AviQtl::kDefaultSampleRate);
    int m_maxBlockSize = AviQtl::kAudioMaxBlockSize;
};

} // namespace AviQtl::Engine::Plugin
