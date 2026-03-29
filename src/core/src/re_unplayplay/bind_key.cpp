#include <cstdint>
#include <cstring>

#include "idadefs.hpp"
#include "types.hpp"

namespace unplayplay {
    [[nodiscard]] Key bind_key(Key decrypted_key, FileId file_id) noexcept {
        std::uint64_t z1, z2, z3, z4;
        memcpy(&z1, decrypted_key.data() + 8, 8);
        memcpy(&z2, decrypted_key.data() + 0, 8);
        memcpy(&z3, file_id.data() + 8, 8);
        memcpy(&z4, file_id.data() + 0, 8);

        for (std::uint8_t i = 0; i < 0x20; ++i) {
            z1 = z4 ^ (__ROR8__(z1, 8) + z2);
            z2 = z1 ^ (__ROL8__(z2, 3));
            z3 = i ^ (__ROR8__(z3, 8) + z4);
            z4 = z3 ^ (__ROL8__(z4, 3));
        }

        Key result = {};
        memcpy(result.data() + 0, &z2, 8);
        memcpy(result.data() + 8, &z1, 8);
        return result;
    }
} // namespace unplayplay
