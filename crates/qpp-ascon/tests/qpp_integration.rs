use qpp_ascon::{BaselineAsconAead128, generate_key, generate_random_nonce};

use qpp_rng_reference::QppRngXorshift;

#[test]
fn qpp_rng_supplies_ascon_key_and_nonce() {
    const INITIAL_SEED: u128 = 0xC0FF_EE00_1234_5678_9ABC_DEF0_1122_3344;

    // Current reference QPP-RNG implementation.
    let mut qpp_rng = QppRngXorshift::from_seed(INITIAL_SEED);

    // QPP-RNG supplies 128 bits for the secret key.
    let key = generate_key(&mut qpp_rng);

    // Experimental random nonce generated from QPP-RNG.
    let nonce = generate_random_nonce(&mut qpp_rng);

    // ASCON itself remains the unmodified NIST-compatible baseline.
    let cipher = BaselineAsconAead128::new(&key);

    let associated_data = b"QPP-RNG-IoT";

    let mut message = *b"QPP-RNG + ASCON";

    let original_message = message;

    let tag = cipher
        .encrypt_in_place(&nonce, associated_data, &mut message)
        .expect("Ascon encryption failed");

    // Ciphertext should not equal plaintext.
    assert_ne!(message, original_message);

    cipher
        .decrypt_in_place(&nonce, associated_data, &mut message, &tag)
        .expect("Ascon authentication/decryption failed");

    assert_eq!(
        message, original_message,
        "decrypted message differs from original"
    );
}
