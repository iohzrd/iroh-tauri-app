use crate::types::{Interaction, Post};
use iroh::{PublicKey, SecretKey, Signature};

/// Produce the canonical bytes for signing a Post.
/// Fields are serialized in a deterministic order, excluding `signature`.
fn post_signing_bytes(post: &Post) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "id": post.id,
        "author": post.author,
        "content": post.content,
        "timestamp": post.timestamp,
        "media": post.media,
        "reply_to": post.reply_to,
        "reply_to_author": post.reply_to_author,
    }))
    .expect("json serialization should not fail")
}

/// Produce the canonical bytes for signing an Interaction.
/// Fields are serialized in a deterministic order, excluding `signature`.
fn interaction_signing_bytes(interaction: &Interaction) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "id": interaction.id,
        "author": interaction.author,
        "kind": interaction.kind,
        "target_post_id": interaction.target_post_id,
        "target_author": interaction.target_author,
        "timestamp": interaction.timestamp,
    }))
    .expect("json serialization should not fail")
}

fn signature_to_hex(sig: &Signature) -> String {
    let bytes = sig.to_bytes();
    let mut hex = String::with_capacity(128);
    for b in &bytes {
        hex.push_str(&format!("{b:02x}"));
    }
    hex
}

fn hex_to_signature(hex: &str) -> Result<Signature, String> {
    if hex.len() != 128 {
        return Err(format!("invalid signature hex length: {}", hex.len()));
    }
    let mut bytes = [0u8; 64];
    for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
        let hi = hex_digit(chunk[0])?;
        let lo = hex_digit(chunk[1])?;
        bytes[i] = (hi << 4) | lo;
    }
    Ok(Signature::from_bytes(&bytes))
}

fn hex_digit(b: u8) -> Result<u8, String> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err(format!("invalid hex digit: {}", b as char)),
    }
}

/// Sign a Post in place using the given secret key.
pub fn sign_post(post: &mut Post, secret_key: &SecretKey) {
    let bytes = post_signing_bytes(post);
    let sig = secret_key.sign(&bytes);
    post.signature = signature_to_hex(&sig);
}

/// Sign an Interaction in place using the given secret key.
pub fn sign_interaction(interaction: &mut Interaction, secret_key: &SecretKey) {
    let bytes = interaction_signing_bytes(interaction);
    let sig = secret_key.sign(&bytes);
    interaction.signature = signature_to_hex(&sig);
}

/// Verify a Post's signature against its author public key.
pub fn verify_post_signature(post: &Post) -> Result<(), String> {
    let sig = hex_to_signature(&post.signature)?;
    let pubkey: PublicKey = post
        .author
        .parse()
        .map_err(|e| format!("invalid author pubkey: {e}"))?;
    let bytes = post_signing_bytes(post);
    pubkey
        .verify(&bytes, &sig)
        .map_err(|_| "signature verification failed".to_string())
}

/// Verify an Interaction's signature against its author public key.
pub fn verify_interaction_signature(interaction: &Interaction) -> Result<(), String> {
    let sig = hex_to_signature(&interaction.signature)?;
    let pubkey: PublicKey = interaction
        .author
        .parse()
        .map_err(|e| format!("invalid author pubkey: {e}"))?;
    let bytes = interaction_signing_bytes(interaction);
    pubkey
        .verify(&bytes, &sig)
        .map_err(|_| "signature verification failed".to_string())
}
