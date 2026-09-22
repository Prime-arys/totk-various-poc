#pragma once

#include <string>

#include "nn/fs.h"
#include "skyline/logger/Logger.hpp"

namespace skyline::logger {
class SdLogger : public Logger {
   public:
    SdLogger(std::string);

    virtual void Initialize();
    virtual bool ShouldFlush() override;
    virtual void SendRaw(void*, size_t);
    virtual std::string FriendlyName() { return "SdLogger"; }

   private:
    nn::fs::FileHandle m_handle = {};
    s64 m_offset = 0;
    bool m_open = false;
};
};  // namespace skyline::logger
