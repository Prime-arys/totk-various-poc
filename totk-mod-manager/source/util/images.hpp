#pragma once

#include <string>

namespace brls
{
class Image;
}

// Pictures loaded in the background: GameBanana screenshots and the mods'
// own thumbnails. Safe if the view is gone by the time they arrive.
namespace images
{

void fromUrl(brls::Image* image, const std::string& url);
void fromMod(brls::Image* image, const std::string& folder);

/** Stops the loader threads (see worker::shutdown). */
void shutdown();

} // namespace images
