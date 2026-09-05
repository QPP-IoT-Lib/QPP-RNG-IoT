//! Ascon-AEAD128 baseline integration for the QPP-RNG-IoT project.
//!
//! This crate intentionally keeps the random-number generator and
//! authenticated encryption algorithm separated.
//!
//! QPP-RNG is responsible for supplying random material such as keys.
//! Ascon-AEAD128 remains the standard NIST SP 800-232 construction.
//!
//! The implementation is `no_std` and uses in-place authenticated
//! encryption so it does not require a heap.

#![no_std]

use ascon_aead128::{
    AsconAead128 as RustCryptoAsconAead128, AsconAead128Key, AsconAead128Nonce, AsconAead128Tag,
    aead::{AeadInOut, KeyInit},
};

use rand_core::Rng;

/// Error returned by the underlying Ascon-AEAD128 implementation.
pub use ascon_aead128::Error as AsconError;

/// Ascon-AEAD128 key size in bytes.
pub const KEY_SIZE: usize = 16;

/// Ascon-AEAD128 nonce size in bytes.
pub const NONCE_SIZE: usize = 16;

/// Ascon-AEAD128 authentication tag size in bytes.
pub const TAG_SIZE: usize = 16;

/// Raw 128-bit Ascon key.
pub type KeyBytes = [u8; KEY_SIZE];

/// Raw 128-bit Ascon nonce.
pub type NonceBytes = [u8; NONCE_SIZE];

/// Raw 128-bit authentication tag.
pub type TagBytes = [u8; TAG_SIZE];

/// Result returned by cryptographic operations.
pub type CryptoResult<T> = core::result::Result<T, AsconError>;

/// Baseline NIST SP 800-232 Ascon-AEAD128 implementation.
///
/// This deliberately wraps the reference RustCrypto implementation
/// without modifying the Ascon permutation, number of rounds,
/// initialization constants, rate, tag or nonce size.
///
/// Optimized IoT implementations can later be compared against this
/// baseline while keeping the same public interface.
pub struct BaselineAsconAead128 {
    cipher: RustCryptoAsconAead128,
}

impl BaselineAsconAead128 {
    /// Creates an Ascon-AEAD128 instance from a 128-bit key.
    #[inline]
    pub fn new(key: &KeyBytes) -> Self {
        let mut ascon_key = AsconAead128Key::default();
        ascon_key.as_mut_slice().copy_from_slice(key);

        Self {
            cipher: RustCryptoAsconAead128::new(&ascon_key),
        }
    }

    /// Encrypts `buffer` in place.
    ///
    /// Before:
    ///
    /// `buffer = plaintext`
    ///
    /// After:
    ///
    /// `buffer = ciphertext`
    ///
    /// The 128-bit authentication tag is returned separately.
    ///
    /// `associated_data` is authenticated but is not encrypted.
    #[inline]
    pub fn encrypt_in_place(
        &self,
        nonce: &NonceBytes,
        associated_data: &[u8],
        buffer: &mut [u8],
    ) -> CryptoResult<TagBytes> {
        let ascon_nonce = nonce_to_ascon(nonce);

        let tag =
            self.cipher
                .encrypt_inout_detached(&ascon_nonce, associated_data, buffer.into())?;

        let mut tag_bytes = [0u8; TAG_SIZE];
        tag_bytes.copy_from_slice(tag.as_slice());

        Ok(tag_bytes)
    }

    /// Authenticates and decrypts `buffer` in place.
    ///
    /// Before:
    ///
    /// `buffer = ciphertext`
    ///
    /// After a successful authentication:
    ///
    /// `buffer = plaintext`
    ///
    /// If the authentication tag is invalid, an error is returned and
    /// the contents of `buffer` must not be used.
    #[inline]
    pub fn decrypt_in_place(
        &self,
        nonce: &NonceBytes,
        associated_data: &[u8],
        buffer: &mut [u8],
        tag: &TagBytes,
    ) -> CryptoResult<()> {
        let ascon_nonce = nonce_to_ascon(nonce);
        let ascon_tag = tag_to_ascon(tag);

        self.cipher
            .decrypt_inout_detached(&ascon_nonce, associated_data, buffer.into(), &ascon_tag)
    }
}

/// Generates a 128-bit Ascon key from any RNG implementing
/// `rand_core::Rng`.
///
/// Every QPP-RNG implementation in this repository implements this
/// interface through `QppRngSource`, so this function can consume
/// QPP-RNG output directly.
#[inline]
pub fn generate_key<R>(rng: &mut R) -> KeyBytes
where
    R: Rng + ?Sized,
{
    let mut key = [0u8; KEY_SIZE];
    rng.fill_bytes(&mut key);
    key
}

/// Generates a random 128-bit nonce from an RNG.
///
/// This function is useful for controlled experiments.
///
/// IMPORTANT:
/// Ascon-AEAD128 requires that a nonce is never reused with the same
/// secret key. Random generation alone does not provide a deterministic
/// uniqueness guarantee across device resets.
///
/// Production IoT firmware should eventually use a persistent
/// counter-based or otherwise uniqueness-preserving nonce strategy.
#[inline]
pub fn generate_random_nonce<R>(rng: &mut R) -> NonceBytes
where
    R: Rng + ?Sized,
{
    let mut nonce = [0u8; NONCE_SIZE];
    rng.fill_bytes(&mut nonce);
    nonce
}

#[inline]
fn nonce_to_ascon(nonce: &NonceBytes) -> AsconAead128Nonce {
    let mut result = AsconAead128Nonce::default();
    result.as_mut_slice().copy_from_slice(nonce);
    result
}

#[inline]
fn tag_to_ascon(tag: &TagBytes) -> AsconAead128Tag {
    let mut result = AsconAead128Tag::default();
    result.as_mut_slice().copy_from_slice(tag);
    result
}
