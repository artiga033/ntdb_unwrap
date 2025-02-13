use hex::ToHex;
use md5::{Digest, Md5 as MD5Hasher};
pub fn md5_hex(data: impl AsRef<[u8]>) -> String {
    let mut hasher = MD5Hasher::new();
    hasher.update(data);
    hasher.finalize().encode_hex()
}
