#pragma once
#include <algorithm>
#include <array>
#include <cassert>
#include <cstddef>
#include <string_view>

namespace unplayplay::util {
    namespace detail {
        [[nodiscard]] constexpr std::uint8_t bit_from_hex(char bit) {
            if (bit >= '0' && bit <= '9') {
                return bit - '0';
            }
            if (bit >= 'A' && bit <= 'F') {
                return bit - 'A' + 10;
            }
            if (bit >= 'a' && bit <= 'f') {
                return bit - 'a' + 10;
            }
            assert(false);
            return 0;
        }

        [[nodiscard]] constexpr bool validate_bytes_view_size(const std::size_t size) {
            return size % 2 == 0;
        }
    } // namespace detail

    template <std::size_t ReservedSize>
    class ByteBuffer {
    public:
        constexpr ByteBuffer() = default;
        ~ByteBuffer() = default;

        constexpr explicit ByteBuffer(const std::uint8_t* pointer, const std::size_t size = ReservedSize) {
            std::copy_n(pointer, std::min(ReservedSize, size), buffer_.begin());
        }

        constexpr explicit ByteBuffer(const std::string_view string_view) {
            assert(detail::validate_bytes_view_size(string_view.size()));

            for (std::size_t i = 0; i < string_view.size(); i += 2) {
                if (i / 2 >= ReservedSize) {
                    continue;
                }

                buffer_[i / 2] = (detail::bit_from_hex(string_view[i]) << 4) | //
                                 detail::bit_from_hex(string_view[i + 1]);
            }
        }

    public:
        [[nodiscard]] constexpr std::uint8_t at(const std::size_t i) const {
            return buffer_.at(i);
        }

        [[nodiscard]] constexpr auto data() {
            return buffer_.data();
        }

        [[nodiscard]] constexpr std::size_t size() const {
            return buffer_.size();
        }

    public:
        [[nodiscard]] constexpr std::uint8_t operator[](const std::size_t i) const {
            return buffer_[i];
        }

        [[nodiscard]] constexpr bool operator==(const ByteBuffer& other) const {
            for (std::size_t i = 0; i < ReservedSize; ++i) {
                if (other.buffer_[i] == this->buffer_[i]) {
                    continue;
                }
                return false;
            }
            return true;
        }

    private:
        std::array<std::uint8_t, ReservedSize> buffer_ = {};
    };
} // namespace unplayplay::util

static_assert(unplayplay::util::ByteBuffer<1>("f1").at(0) == 0xF1);
