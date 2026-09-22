#pragma once

#include <vector>

#include "skyline/logger/Logger.hpp"

namespace skyline::logger {

// Fans a log line out to every configured sink (kernel / SD card / TCP).
class CompositeLogger : public Logger {
   public:
    void AddSink(Logger* sink) { m_sinks.push_back(sink); }
    bool Empty() const { return m_sinks.empty(); }

    virtual void Initialize() override;
    virtual bool ShouldFlush() override;
    virtual void SendRaw(void*, size_t) override;
    virtual std::string FriendlyName() override;

   private:
    std::vector<Logger*> m_sinks;
};

};  // namespace skyline::logger
