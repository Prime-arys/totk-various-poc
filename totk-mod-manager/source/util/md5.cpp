#include "util/md5.hpp"

#include <algorithm>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <vector>

namespace md5
{

namespace
{
    // RFC 1321.
    struct Context
    {
        uint32_t state[4] = { 0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476 };
        uint64_t length   = 0;
        uint8_t buffer[64];
        size_t used = 0;
    };

    const uint32_t K[64] = {
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613, 0xfd469501,
        0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821,
        0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d, 0x02441453, 0xd8a1e681, 0xe7d3fbc8,
        0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed, 0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a,
        0xfffa3942, 0x8771f681, 0x6d9d6122, 0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70,
        0x289b7ec6, 0xeaa127fa, 0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665,
        0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb, 0xeb86d391,
    };

    const uint8_t SHIFT[64] = {
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20,
        4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    };

    uint32_t rotate(uint32_t value, uint8_t bits)
    {
        return (value << bits) | (value >> (32 - bits));
    }

    void block(Context& context, const uint8_t* data)
    {
        uint32_t words[16];
        for (int i = 0; i < 16; i++)
            words[i] = (uint32_t)data[i * 4] | ((uint32_t)data[i * 4 + 1] << 8) | ((uint32_t)data[i * 4 + 2] << 16)
                | ((uint32_t)data[i * 4 + 3] << 24);

        uint32_t a = context.state[0], b = context.state[1], c = context.state[2], d = context.state[3];
        for (int i = 0; i < 64; i++)
        {
            uint32_t f;
            int g;
            if (i < 16)
            {
                f = (b & c) | (~b & d);
                g = i;
            }
            else if (i < 32)
            {
                f = (d & b) | (~d & c);
                g = (5 * i + 1) % 16;
            }
            else if (i < 48)
            {
                f = b ^ c ^ d;
                g = (3 * i + 5) % 16;
            }
            else
            {
                f = c ^ (b | ~d);
                g = (7 * i) % 16;
            }
            uint32_t next = d;
            d             = c;
            c             = b;
            b             = b + rotate(a + f + K[i] + words[g], SHIFT[i]);
            a             = next;
        }
        context.state[0] += a;
        context.state[1] += b;
        context.state[2] += c;
        context.state[3] += d;
    }

    void update(Context& context, const uint8_t* data, size_t size)
    {
        context.length += size;
        if (context.used)
        {
            size_t take = std::min(size, 64 - context.used);
            std::memcpy(context.buffer + context.used, data, take);
            context.used += take;
            data += take;
            size -= take;
            if (context.used < 64)
                return;
            block(context, context.buffer);
            context.used = 0;
        }
        while (size >= 64)
        {
            block(context, data);
            data += 64;
            size -= 64;
        }
        std::memcpy(context.buffer, data, size);
        context.used = size;
    }

    std::string finish(Context& context)
    {
        uint64_t bits = context.length * 8;
        uint8_t padding[64] = { 0x80 };
        size_t padLength = context.used < 56 ? 56 - context.used : 120 - context.used;
        update(context, padding, padLength);
        uint8_t length[8];
        for (int i = 0; i < 8; i++)
            length[i] = (uint8_t)(bits >> (8 * i));
        update(context, length, 8);

        static const char* digits = "0123456789abcdef";
        std::string hex;
        for (uint32_t word : context.state)
            for (int i = 0; i < 4; i++)
            {
                uint8_t byte = (uint8_t)(word >> (8 * i));
                hex.push_back(digits[byte >> 4]);
                hex.push_back(digits[byte & 15]);
            }
        return hex;
    }
} // namespace

std::string ofFile(const std::string& path)
{
    FILE* file = fopen(path.c_str(), "rb");
    if (!file)
        return "";
    Context context;
    std::vector<uint8_t> buffer(1024 * 1024);
    size_t read;
    while ((read = fread(buffer.data(), 1, buffer.size(), file)) > 0)
        update(context, buffer.data(), read);
    bool failed = ferror(file) != 0;
    fclose(file);
    return failed ? "" : finish(context);
}

std::string ofText(const std::string& text)
{
    Context context;
    update(context, (const uint8_t*)text.data(), text.size());
    return finish(context);
}

} // namespace md5
