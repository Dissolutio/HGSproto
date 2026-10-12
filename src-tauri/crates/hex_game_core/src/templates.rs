pub struct CubeOffset {
    pub q: i32,
    pub r: i32,
}

const ORIGIN: CubeOffset = CubeOffset { q: 0, r: 0 };
const EAST: CubeOffset = CubeOffset { q: 1, r: 0 };
const SE: CubeOffset = CubeOffset { q: 0, r: 1 };
const SW: CubeOffset = CubeOffset { q: -1, r: 1 };
const WEST: CubeOffset = CubeOffset { q: -1, r: 0 };
const NW: CubeOffset = CubeOffset { q: 0, r: -1 };
const NE: CubeOffset = CubeOffset { q: 1, r: -1 };
const _: (CubeOffset, CubeOffset, CubeOffset) = (WEST, NW, NE);

pub fn get_template_offsets(inventory_id: &str) -> &'static [CubeOffset] {
    match inventory_id {
        "straight3" | "castleArch" | "shipWall" => return &STRAIGHT_3,
        "straight4" | "roadWall" => return &STRAIGHT_4,
        "wallWalk7" => return &WALL_WALK_7,
        "wallWalk9" => return &WALL_WALK_9,
        _ => {}
    }

    let numeric_key: String = inventory_id
        .chars()
        .filter(|character| character.is_ascii_digit())
        .collect();
    match numeric_key.as_str() {
        "1" | "10" | "11" | "12" | "13" | "14" | "15" | "16" => &BASIC_1,
        "2" => &BASIC_2,
        "3" => &BASIC_3,
        "4" => &GLACIER_4,
        "5" => &STRAIGHT_5,
        "6" => &GLACIER_6,
        "7" => &BASIC_7,
        "24" => &BASIC_24,
        _ => &BASIC_1,
    }
}

const BASIC_1: [CubeOffset; 1] = [ORIGIN];
const BASIC_2: [CubeOffset; 2] = [ORIGIN, EAST];
const BASIC_3: [CubeOffset; 3] = [ORIGIN, EAST, SE];
const GLACIER_4: [CubeOffset; 4] = [ORIGIN, EAST, SE, CubeOffset { q: 1, r: 1 }];
const GLACIER_6: [CubeOffset; 6] = [
    ORIGIN,
    EAST,
    SE,
    SW,
    CubeOffset { q: 2, r: 0 },
    CubeOffset { q: 1, r: 1 },
];
const BASIC_7: [CubeOffset; 7] = [
    ORIGIN,
    EAST,
    SE,
    SW,
    CubeOffset { q: 1, r: 1 },
    CubeOffset { q: -1, r: 2 },
    CubeOffset { q: 0, r: 2 },
];
const STRAIGHT_3: [CubeOffset; 3] = [ORIGIN, EAST, CubeOffset { q: 2, r: 0 }];
const STRAIGHT_4: [CubeOffset; 4] = [
    ORIGIN,
    EAST,
    CubeOffset { q: 2, r: 0 },
    CubeOffset { q: 3, r: 0 },
];
const STRAIGHT_5: [CubeOffset; 5] = [
    ORIGIN,
    EAST,
    CubeOffset { q: 2, r: 0 },
    CubeOffset { q: 3, r: 0 },
    CubeOffset { q: 4, r: 0 },
];
const WALL_WALK_7: [CubeOffset; 7] = [
    ORIGIN,
    EAST,
    SE,
    CubeOffset { q: 2, r: 0 },
    CubeOffset { q: 3, r: 0 },
    CubeOffset { q: 1, r: 1 },
    CubeOffset { q: 2, r: 1 },
];
const WALL_WALK_9: [CubeOffset; 9] = [
    ORIGIN,
    EAST,
    SE,
    CubeOffset { q: 2, r: 0 },
    CubeOffset { q: 3, r: 0 },
    CubeOffset { q: 1, r: 1 },
    CubeOffset { q: 2, r: 1 },
    CubeOffset { q: 4, r: 0 },
    CubeOffset { q: 3, r: 1 },
];
const BASIC_24: [CubeOffset; 24] = [
    ORIGIN,
    EAST,
    SE,
    SW,
    CubeOffset { q: 1, r: 1 },
    CubeOffset { q: -1, r: 2 },
    CubeOffset { q: 0, r: 2 },
    CubeOffset { q: 1, r: 2 },
    CubeOffset { q: -2, r: 3 },
    CubeOffset { q: -1, r: 3 },
    CubeOffset { q: 0, r: 3 },
    CubeOffset { q: 1, r: 3 },
    CubeOffset { q: 2, r: 3 },
    CubeOffset { q: -2, r: 4 },
    CubeOffset { q: -1, r: 4 },
    CubeOffset { q: 0, r: 4 },
    CubeOffset { q: 1, r: 4 },
    CubeOffset { q: 2, r: 4 },
    CubeOffset { q: -3, r: 5 },
    CubeOffset { q: -2, r: 5 },
    CubeOffset { q: -1, r: 5 },
    CubeOffset { q: 0, r: 5 },
    CubeOffset { q: 1, r: 5 },
    CubeOffset { q: 2, r: 5 },
];
