#pragma once
#include "types.hpp"

namespace unplayplay {
    [[nodiscard]] Key decrypt_key(Key key_basis) noexcept;
    [[nodiscard]] Key bind_key(Key decrypted_key, FileId file_id) noexcept;

    [[nodiscard]] inline Token get_token() noexcept {
        return Token{detail::kPlayPlayToken.data()};
    }

    [[nodiscard]] inline Key decrypt_and_bind_key(Key key_basis, FileId file_id) noexcept {
        const auto decrypted_key = decrypt_key(key_basis);
        return bind_key(decrypted_key, file_id);
    }
} // namespace unplayplay
