#include "audio_plugin_chain.hpp"

#include <utility>

namespace AviQtl::Engine::Plugin {

void AudioPluginChain::add(std::unique_ptr<IAudioPlugin> plugin, bool enabled) {
    plugin->prepare(m_sampleRate, m_maxBlockSize);
    m_plugins.push_back({std::move(plugin), enabled});
}

void AudioPluginChain::clear() { m_plugins.clear(); }

void AudioPluginChain::process(float *buf, int frameCount) {
    for (const auto &p : std::as_const(m_plugins)) {
        if (p.enabled) {
            p.plugin->process(buf, frameCount);
        }
    }
}

auto AudioPluginChain::count() const -> int { return static_cast<int>(m_plugins.size()); }

auto AudioPluginChain::get(int index) const -> IAudioPlugin * { return (index >= 0 && std::cmp_less(index, m_plugins.size())) ? m_plugins[static_cast<std::size_t>(index)].plugin.get() : nullptr; }

} // namespace AviQtl::Engine::Plugin
