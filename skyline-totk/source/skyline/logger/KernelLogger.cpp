#include "skyline/logger/KernelLogger.hpp"

#include <string.h>

#ifdef __cplusplus
extern "C" {
#endif

#include "skyline/nx/kernel/svc.h"

#ifdef __cplusplus
}
#endif

namespace skyline::logger {

KernelLogger::KernelLogger() {}

void KernelLogger::Initialize() {
    // nothing to do
}

void KernelLogger::SendRaw(void* data, size_t size) {
    svcOutputDebugString((const char*)data, size);
};

bool KernelLogger::ShouldFlush() {
    return true;
}

};  // namespace skyline::logger