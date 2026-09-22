#include "skyline/logger/CompositeLogger.hpp"

namespace skyline::logger {

void CompositeLogger::Initialize() {
    for (auto* sink : m_sinks) sink->Initialize();
}

bool CompositeLogger::ShouldFlush() {
    for (auto* sink : m_sinks)
        if (sink->ShouldFlush()) return true;
    return !m_sinks.empty();
}

void CompositeLogger::SendRaw(void* data, size_t size) {
    for (auto* sink : m_sinks) sink->SendRaw(data, size);
}

std::string CompositeLogger::FriendlyName() {
    std::string name;
    for (auto* sink : m_sinks) {
        if (!name.empty()) name += "+";
        name += sink->FriendlyName();
    }
    return name.empty() ? "NoLogger" : name;
}

};  // namespace skyline::logger
