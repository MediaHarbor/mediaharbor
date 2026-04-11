#pragma once
#include <cstdint>

template <class Ty>
constexpr Ty __ROL__(Ty value, int count) noexcept {
    const std::uint32_t nbits = sizeof(Ty) * 8;

    if (count > 0) {
        count %= nbits;
        Ty high = value >> (nbits - count);
        if (Ty(-1) < 0) // signed value
            high &= ~((Ty(-1) << count));
        value <<= count;
        value |= high;
    } else {
        count = -count % nbits;
        Ty low = value << (nbits - count);
        value >>= count;
        value |= low;
    }
    return value;
}

constexpr inline std::uint32_t __ROL4__(std::uint32_t value, int count) noexcept {
    return __ROL__(value, count);
}

constexpr inline std::uint32_t __ROR4__(std::uint32_t value, int count) noexcept {
    return __ROL__(value, -count);
}

constexpr inline std::uint64_t __ROL8__(std::uint64_t value, int count) noexcept {
    return __ROL__(value, count);
}

constexpr inline std::uint64_t __ROR8__(std::uint64_t value, int count) noexcept {
    return __ROL__(value, -count);
}

template <typename A>
constexpr unsigned char bit_test(A* a, int b) {
    auto const bits{reinterpret_cast<unsigned char const*>(a)};
    auto const value{bits[b >> 3]};
    auto const mask{(unsigned char)(1 << (b & 7))};
    return (value & mask) != 0;
}

#define LAST_IND(x, part_type) (sizeof(x) / sizeof(part_type) - 1)
#if defined(__BYTE_ORDER) && __BYTE_ORDER == __BIG_ENDIAN
    #define LOW_IND(x, part_type) LAST_IND(x, part_type)
    #define HIGH_IND(x, part_type) 0
#else
    #define HIGH_IND(x, part_type) LAST_IND(x, part_type)
    #define LOW_IND(x, part_type) 0
#endif

#define DWORDn(x, n) (*((uint32_t*)&(x) + n))
#define LODWORD(x) DWORDn(x, LOW_IND(x, uint32_t))
#define HIDWORD(x) DWORDn(x, HIGH_IND(x, uint32_t))

#define BYTEn(x, n) (*((uint8_t*)&(x) + n))
#define BYTE1(x) BYTEn(x, 1)
#define BYTE2(x) BYTEn(x, 2)
#define LOBYTE(x) BYTEn(x, LOW_IND(x, uint8_t))
#define HIBYTE(x) BYTEn(x, HIGH_IND(x, uint8_t))

#define WORDn(x, n) (*((uint16_t*)&(x) + n))
#define HIWORD(x) WORDn(x, HIGH_IND(x, uint16_t))

#define __PAIR64__(high, low) (((uint64_t)(high) << 32) | (uint32_t)(low))
