const base32Alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

export function decodeBase32(value: string): Uint8Array {
  let buffer = 0;
  let bits = 0;
  const bytes: number[] = [];
  for (const character of value.toUpperCase().replace(/=+$/, "")) {
    const digit = base32Alphabet.indexOf(character);
    if (digit < 0) throw new Error("Invalid base32 character: " + character);
    buffer = (buffer << 5) | digit;
    bits += 5;
    if (bits >= 8) {
      bits -= 8;
      bytes.push((buffer >> bits) & 0xff);
    }
  }
  return new Uint8Array(bytes);
}

export async function totpCodeAt(
  secret: string,
  unixSeconds = Math.floor(Date.now() / 1000),
): Promise<string> {
  const key = await crypto.subtle.importKey(
    "raw",
    decodeBase32(secret) as unknown as BufferSource,
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  );
  const counter = Math.floor(unixSeconds / 30);
  const message = new ArrayBuffer(8);
  new DataView(message).setBigUint64(0, BigInt(counter));
  const digest = new Uint8Array(await crypto.subtle.sign("HMAC", key, message));
  const offset = digest[digest.length - 1] & 0x0f;
  const binary = ((digest[offset] & 0x7f) << 24) |
    (digest[offset + 1] << 16) |
    (digest[offset + 2] << 8) |
    digest[offset + 3];
  return String(binary % 1_000_000).padStart(6, "0");
}
