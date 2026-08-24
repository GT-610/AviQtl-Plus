#include "audio_plugin_chain.hpp"

#include <utility>

namespace AviQtl::Engine::Plugin {

void AudioPluginChain::add(std::unique_ptr<IAudioPlugin> plugin, bool enabled) {
    std::lock_guard lock(m_mutex);
    plugin->prepare(m_sampleRate, m_maxBlockSize);
    m_plugins.push_back({std::move(plugin), enabled});
}

void AudioPluginChain::clear() {
    std::lock_guard lock(m_mutex);
    m_plugins.clear();
}

void AudioPluginChain::prepare(double sampleRate) {
    std::lock_guard lock(m_mutex);
    m_sampleRate = sampleRate;
    for (const auto &entry : m_plugins) {
        entry.plugin->prepare(m_sampleRate, m_maxBlockSize);
    }
}

void AudioPluginChain::process(float *buf, int frameCount) {
    std::lock_guard lock(m_mutex);
    for (const auto &p : std::as_const(m_plugins)) {
        if (p.enabled) {
            p.plugin->process(buf, frameCount);
        }
    }
}

auto AudioPluginChain::count() const -> int {
    std::lock_guard lock(m_mutex);
    return static_cast<int>(m_plugins.size());
}

auto AudioPluginChain::describe(int index) const -> std::optional<Description> {
    std::lock_guard lock(m_mutex);
    if (index < 0 || !std::cmp_less(index, m_plugins.size())) {
        return std::nullopt;
    }
    const auto &plugin = m_plugins[static_cast<std::size_t>(index)].plugin;
    Description description{.name = plugin->name(), .format = plugin->format()};
    const int parameterCount = plugin->paramCount();
    description.parameters.reserve(static_cast<std::size_t>(std::max(parameterCount, 0)));
    description.values.reserve(static_cast<std::size_t>(std::max(parameterCount, 0)));
    for (int parameter = 0; parameter < parameterCount; ++parameter) {
        description.parameters.push_back(plugin->getParamInfo(parameter));
        description.values.push_back(plugin->getParam(parameter));
    }
    return description;
}

bool AudioPluginChain::setParameter(int pluginIndex, int parameterIndex, float value) {
    std::lock_guard lock(m_mutex);
    if (pluginIndex < 0 || !std::cmp_less(pluginIndex, m_plugins.size())) {
        return false;
    }
    auto &plugin = m_plugins[static_cast<std::size_t>(pluginIndex)].plugin;
    if (parameterIndex < 0 || parameterIndex >= plugin->paramCount()) {
        return false;
    }
    plugin->setParam(parameterIndex, value);
    return true;
}

} // namespace AviQtl::Engine::Plugin
