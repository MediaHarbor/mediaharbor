#include <cstdint>
#include <cstring>
#include "unplayplay.hpp"

extern "C" {
    void playplay_decrypt_and_bind_key(
        const uint8_t* obfuscated_key,   // 16 bytes in
        const uint8_t* file_id,           // 20 bytes in
        uint8_t* output                   // 16 bytes out
    ) {
        unplayplay::Key key(obfuscated_key, 16);
        unplayplay::FileId fid(file_id, 20);
        auto result = unplayplay::decrypt_and_bind_key(key, fid);
        std::memcpy(output, result.data(), 16);
    }

    void playplay_get_token(uint8_t* output) {   // 16 bytes out
        auto token = unplayplay::get_token();
        std::memcpy(output, token.data(), 16);
    }
}
