unsafe extern "C" {
    fn playplay_decrypt_and_bind_key(obfuscated_key: *const u8, file_id: *const u8, output: *mut u8);
    fn playplay_get_token(output: *mut u8);
}

pub fn decrypt_and_bind_key(obfuscated_key: &[u8; 16], file_id: &[u8; 20]) -> [u8; 16] {
    let mut output = [0u8; 16];
    unsafe {
        playplay_decrypt_and_bind_key(
            obfuscated_key.as_ptr(),
            file_id.as_ptr(),
            output.as_mut_ptr(),
        );
    }
    output
}

pub fn get_token() -> [u8; 16] {
    let mut output = [0u8; 16];
    unsafe { playplay_get_token(output.as_mut_ptr()); }
    output
}
