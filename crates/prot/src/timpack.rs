pub fn is_tim_pack(blob: &[u8]) -> bool {
    if blob.len() < 12 {
        return false;
    }
    if !(blob[3] == 0x01 && blob[2] < 0x10) {
        return false;
    }
    let tim_num = i32::from_le_bytes(blob[4..8].try_into().unwrap());
    if tim_num <= 0 {
        return false;
    }
    let table_end = 8usize.saturating_add(4usize.saturating_mul(tim_num as usize));
    table_end <= blob.len()
}

pub fn unpack(blob: &[u8]) -> Vec<Vec<u8>> {
    if !is_tim_pack(blob) {
        return Vec::new();
    }
    let tim_num = i32::from_le_bytes(blob[4..8].try_into().unwrap()) as usize;
    let mut offsets = Vec::with_capacity(tim_num + 1);
    for x in 0..tim_num {
        let entry = i32::from_le_bytes(blob[8 + 4 * x..12 + 4 * x].try_into().unwrap());
        let off = (entry as i64) * 4 + 4;
        if off < 0 || off as usize > blob.len() {
            continue;
        }
        offsets.push(off as usize);
    }
    offsets.sort_unstable();
    offsets.dedup();
    offsets.push(blob.len());

    let mut out = Vec::with_capacity(offsets.len().saturating_sub(1));
    for w in offsets.windows(2) {
        let (s, e) = (w[0], w[1]);
        if s < e && e <= blob.len() {
            out.push(blob[s..e].to_vec());
        }
    }
    out
}

pub fn detected_ext(item: &[u8]) -> &'static str {
    if !item.is_empty() && item[0] == 0x10 {
        "TIM"
    } else {
        "BIN"
    }
}
