#include "ecs.hpp"
#include "ecs_profiler.hpp"
#include <QDebug>
#include <cassert>
#include <cmath>

namespace AviQtl::Engine::Timeline {

void ECS::markDirty(int clipId) {
    for (int i = 1; i <= 2; ++i) {
        auto &df = m_dirtyFlags[(m_editIndex + i) % 3];
        if (!df.dirty.test(static_cast<std::size_t>(clipId))) {
            df.dirty.set(static_cast<std::size_t>(clipId));
            df.dirtyIds.push_back(clipId);
        }
    }
    ECS_PROF_INC(dirtyBitSetCount);
}

ECS::ECS() : m_editIndex(1) {
    for (auto &buffer : m_buffers)
        buffer = std::make_shared<ECSState>();
    for (auto &f : m_dirtyFlags)
        f.fullSync = true;
}

auto ECS::instance() -> ECS & {
    static ECS inst;
    return inst;
}

void ECS::syncClipIds(const std::bitset<MAX_CLIP_ID> &aliveFlags) {
    auto &editState = *m_buffers[m_editIndex];
    bool changed = false;
    changed |= editState.renderStates.syncAlive(aliveFlags);
    changed |= editState.audioStates.syncAlive(aliveFlags);

    if (changed) {
        m_dirtyFlags[(m_editIndex + 1) % 3].fullSync = true;
        m_dirtyFlags[(m_editIndex + 2) % 3].fullSync = true;
    }
}

void ECS::updateClipState(int clipId, int layer, double time, int startFrame, int durationFrames) {
    assert(clipId >= 0 && clipId < MAX_CLIP_ID);
    auto &editState = *m_buffers[m_editIndex];
    auto *ptr = editState.renderStates.find(clipId);
    if (!ptr) {
        m_dirtyFlags[(m_editIndex + 1) % 3].fullSync = true;
        m_dirtyFlags[(m_editIndex + 2) % 3].fullSync = true;
        ptr = &editState.renderStates[clipId];
    }
    auto &render = *ptr;
    bool changed = (render.clipId != clipId) || (render.layer != layer) || (std::abs(render.timePosition - time) > 0.001) || (render.startFrame != startFrame) || (render.durationFrames != durationFrames);
    if (changed) {
        render.clipId = clipId;
        render.layer = layer;
        render.timePosition = time;
        render.startFrame = startFrame;
        render.durationFrames = durationFrames;
        editState.renderGraphGeneration++;
    }

    if (changed) {
        markDirty(clipId);
    }
}

void ECS::updateAudioClipState(int clipId, const AudioComponent &audio) {
    assert(clipId >= 0 && clipId < MAX_CLIP_ID);
    auto &editState = *m_buffers[m_editIndex];
    auto *ptr = editState.audioStates.find(clipId);
    if (!ptr) {
        m_dirtyFlags[(m_editIndex + 1) % 3].fullSync = true;
        m_dirtyFlags[(m_editIndex + 2) % 3].fullSync = true;
        ptr = &editState.audioStates[clipId];
    }
    AudioComponent next = audio;
    next.clipId = clipId;
    if (*ptr != next) {
        *ptr = next;
        markDirty(clipId);
    }
}

void ECS::updateRenderState(int clipId, const RenderComponent &render) {
    assert(clipId >= 0 && clipId < MAX_CLIP_ID);
    auto &editState = *m_buffers[m_editIndex];
    auto *current = editState.renderStates.find(clipId);
    if (current == nullptr || *current != render) {
        editState.renderStates[clipId] = render;
        markDirty(clipId);
    }
}

void ECS::clearEffectParams() {
    m_buffers[m_editIndex]->effectParams.clear();
}

void ECS::commit() {
    ECS_PROF_INC(commitCount);
    std::lock_guard lock(m_snapshotMutex);

    const int justWritten = m_editIndex;
    const int active = m_activeIndex;
    const int pending = m_pendingIndex;

    // 次に書き込むバッファを選択 (justWritten, active, pending を避ける)
    int next = -1;
    for (int c = 0; c < 3; ++c) {
        if (c == justWritten || c == active || (pending != -1 && c == pending))
            continue;
        next = c;
        break;
    }
    // 全て埋まっている場合（理論上稀）は、pending を上書きする
    if (next == -1)
        next = (pending != -1) ? pending : (justWritten + 1) % 3;

    m_editIndex = next;
    bool replacedLeasedBuffer = false;
    if (m_buffers[m_editIndex].use_count() > 1) {
        m_buffers[m_editIndex] = std::make_shared<ECSState>();
        replacedLeasedBuffer = true;
    }

    auto &df = m_dirtyFlags[m_editIndex];
    if (df.fullSync || replacedLeasedBuffer) {
        *m_buffers[m_editIndex] = *m_buffers[justWritten];
        df.fullSync = false;
        df.dirty.reset();
        df.dirtyIds.clear();
    } else {
        const auto &src = *m_buffers[justWritten];
        auto &dst = *m_buffers[m_editIndex];
        dst.renderGraphGeneration = src.renderGraphGeneration;

        for (int id : df.dirtyIds) {
            if (const auto *s = src.renderStates.find(id))
                dst.renderStates[id] = *s;
            if (const auto *s = src.audioStates.find(id))
                dst.audioStates[id] = *s;
        }
        df.dirty.reset();
        df.dirtyIds.clear();
    }

    m_buffers[m_editIndex]->effectParams = m_buffers[justWritten]->effectParams;

    m_pendingIndex = justWritten;
}

auto ECS::getSnapshot() const -> std::shared_ptr<const ECSState> {
    std::lock_guard lock(m_snapshotMutex);
    const int pending = m_pendingIndex;
    if (pending != -1) {
        m_pendingIndex = -1;
        m_activeIndex = pending;
    }
    return m_buffers[m_activeIndex];
}

} // namespace AviQtl::Engine::Timeline
