use qpp_ascon::BaselineAsconAead128;

const KEY: [u8; 16] = [
    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F,
];

const NONCE: [u8; 16] = [
    0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1A, 0x1B, 0x1C, 0x1D, 0x1E, 0x1F,
];

#[test]
fn nist_sp800_232_known_answer_test() {
    // Official Ascon-AEAD128 KAT:
    //
    // Count = 35
    // PT    = 20
    // AD    = 30
    //
    // Combined output:
    // CT = 962B8016836C75A7D86866588CA245D886
    //
    // First byte  = ciphertext
    // Remaining 16 bytes = authentication tag

    let cipher = BaselineAsconAead128::new(&KEY);

    let associated_data = [0x30];

    let mut message = [0x20];

    let tag = cipher
        .encrypt_in_place(&NONCE, &associated_data, &mut message)
        .expect("Ascon encryption failed");

    assert_eq!(
        message,
        [0x96],
        "ciphertext does not match the official KAT"
    );

    assert_eq!(
        tag,
        [
            0x2B, 0x80, 0x16, 0x83, 0x6C, 0x75, 0xA7, 0xD8, 0x68, 0x66, 0x58, 0x8C, 0xA2, 0x45,
            0xD8, 0x86,
        ],
        "authentication tag does not match the official KAT"
    );

    cipher
        .decrypt_in_place(&NONCE, &associated_data, &mut message, &tag)
        .expect("Ascon decryption failed");

    assert_eq!(
        message,
        [0x20],
        "decrypted plaintext does not match original plaintext"
    );
}

#[test]
fn modified_tag_is_rejected() {
    let cipher = BaselineAsconAead128::new(&KEY);

    let associated_data = b"QPP-RNG-IoT";
    let mut message = *b"ASCON test";

    let mut tag = cipher
        .encrypt_in_place(&NONCE, associated_data, &mut message)
        .expect("Ascon encryption failed");

    // Simulate corruption or manipulation of the authentication tag.
    tag[0] ^= 0x01;

    let result = cipher.decrypt_in_place(&NONCE, associated_data, &mut message, &tag);

    assert!(
        result.is_err(),
        "Ascon accepted a modified authentication tag"
    );
}
