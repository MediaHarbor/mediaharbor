#pragma once
#include "constants.hpp"
#include "util.hpp"

namespace unplayplay {
    using Key = util::ByteBuffer<unplayplay::detail::kKeySize>;
    using FileId = util::ByteBuffer<unplayplay::detail::kFileIdSize>;
    using Token = util::ByteBuffer<unplayplay::detail::kTokenSize>;
} // namespace unplayplay
