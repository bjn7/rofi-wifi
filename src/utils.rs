pub const UUIDV4_PREFIX: &str = "12345678";
use rand;
pub fn generate_uuid() -> String {
    let mut ran_bytes: [u8; 16] = rand::random();
    ran_bytes[6] = (ran_bytes[6] & 0x0F) | 0x40; //v4 in the most significant
    ran_bytes[8] = (ran_bytes[8] & 0x3F) | 0x80; // two most significant bits to 10

    format!(
        "{}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        UUIDV4_PREFIX,
        ran_bytes[4],
        ran_bytes[5],
        ran_bytes[6],
        ran_bytes[7],
        ran_bytes[8],
        ran_bytes[9],
        ran_bytes[10],
        ran_bytes[11],
        ran_bytes[12],
        ran_bytes[13],
        ran_bytes[14],
        ran_bytes[15],
    )
}
